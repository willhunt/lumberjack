//! Channels worked out from other channels rather than measured.
//!
//! A sensor reports volts; the thing you care about is pressure or flow. A
//! calculated channel applies an equation to one or more measured channels and
//! records the result alongside them, so what gets stored is the quantity you
//! actually wanted.
//!
//! These are not a device in the running sense. Devices own hardware and are
//! read on their own threads, each with exclusive access; a calculated channel
//! needs to see *other* devices' data, which that arrangement deliberately
//! prevents. So it is a device in the configuration and in the results, but the
//! work happens where every device's data already converges: the collecting
//! loop, between the devices and the sink.

use crate::channel::ChannelInfo;
use crate::datapoint::DataPoint;
use crate::device::DeviceInfo;
use crate::session::DeviceEvent;
use crate::storage::Batch;
use crate::{ Error, Result };
use serde::{ Deserialize, Serialize };
use std::collections::BTreeMap;

type Tree = evalexpr::Node<evalexpr::DefaultNumericTypes>;
type Context = evalexpr::HashMapContext<evalexpr::DefaultNumericTypes>;

/// Which measured channel an equation takes a value from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChannelRef {
    pub device: String,
    pub channel: String,
}

impl std::fmt::Display for ChannelRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "{}/{}", self.device, self.channel)
    }
}

/// One calculated channel: what it is, where its values come from, and the
/// equation that turns them into it.
///
/// Inputs are named rather than referred to inline, because channel names have
/// spaces in them and quoting those inside an expression is miserable. The
/// short name is what appears in the equation.
#[derive(Serialize, Deserialize, Clone)]
pub struct CalculatedChannel {
    #[serde(flatten)]
    pub info: ChannelInfo,
    pub inputs: BTreeMap<String, ChannelRef>,
    pub equation: String,
}

impl CalculatedChannel {
    /// Check this channel on its own, without building a whole calculator.
    ///
    /// Equations are strings read at run time, so a program embedding this can
    /// let someone write one while it is running: a UI can call this as they
    /// type and say what is wrong before anything is recorded. Nothing about an
    /// equation is fixed when lumberdaq is built.
    ///
    /// The same checks a run would make, so passing here means it will build.
    pub fn validate(&self) -> Result<()> {
        compile(self).map(|_| ())
    }
}

/// The device calculated channels belong to.
#[derive(Serialize, Deserialize, Clone)]
pub struct CalculatedDevice {
    pub info: DeviceInfo,
    pub channels: Vec<CalculatedChannel>,
}

/// A calculated channel with its equation compiled and its input resolved.
struct Compiled {
    info: ChannelInfo,
    tree: Tree,
    /// The variable name the equation uses, and the channel it reads.
    ///
    /// One input only for now. Several inputs sampled at different rates never
    /// share a timestamp, so combining them needs a rule about which value of
    /// the slower one to use, and that is a decision worth making on its own
    /// rather than falling out of an implementation.
    variable: String,
    source: ChannelRef,
}

/// Turns measured batches into calculated ones.
pub struct Calculator {
    config: CalculatedDevice,
    compiled: Vec<Compiled>,
}

impl Calculator {
    /// Compile every equation, so a bad one is refused before a run starts
    /// rather than failing partway through.
    pub fn from_config(config: CalculatedDevice) -> Result<Calculator> {
        let mut compiled = Vec::with_capacity(config.channels.len());
        for channel in config.channels.iter() {
            compiled.push(compile(channel)?);
        }
        Ok(Calculator { config: config, compiled: compiled })
    }

    pub fn config(&self) -> CalculatedDevice {
        self.config.clone()
    }

    pub fn device_name(&self) -> &str {
        &self.config.info.name
    }

    /// Every channel this calculator reads from.
    ///
    /// Used to check a setup before running it: an equation naming a channel
    /// that does not exist is a typo, and better caught now than by silently
    /// producing nothing for the whole run.
    pub fn sources(&self) -> Vec<ChannelRef> {
        self.compiled.iter().map(|channel| channel.source.clone()).collect()
    }

    /// Work out whatever this batch makes possible.
    ///
    /// A batch of measured data may feed several calculated channels, or none.
    /// Failures are reported and skipped rather than ending the run: an
    /// equation that divides by zero on one sample should not stop the
    /// recording of everything else.
    pub fn apply(&mut self, batch: &Batch, on_event: &mut dyn FnMut(DeviceEvent)) -> Vec<Batch> {
        let mut produced: Vec<Batch> = Vec::new();

        for channel in self.compiled.iter() {
            if channel.source.device != batch.device || channel.source.channel != batch.channel {
                continue;
            }

            let mut context = fresh_context();
            let mut datapoints: Vec<DataPoint> = Vec::with_capacity(batch.datapoints.len());
            let mut skipped = 0usize;
            let mut first_failure: Option<String> = None;

            for datapoint in batch.datapoints.iter() {
                match evaluate(&channel.tree, &mut context, &channel.variable, datapoint.value) {
                    Ok(value) => datapoints.push(DataPoint {
                        // The input's own timestamp. A calculated value happened
                        // when the measurement it came from happened, not when
                        // the arithmetic was done.
                        datetime: datapoint.datetime,
                        value: value,
                    }),
                    Err(reason) => {
                        skipped += 1;
                        if first_failure.is_none() {
                            first_failure = Some(reason);
                        }
                    }
                }
            }

            if let Some(reason) = first_failure {
                on_event(DeviceEvent::Problem {
                    device: self.config.info.name.clone(),
                    error: Error::EquationFailed {
                        channel: channel.info.name.clone(),
                        equation: channel_equation(&self.config, &channel.info.name),
                        skipped: skipped,
                        reason: reason,
                    },
                });
            }

            if !datapoints.is_empty() {
                produced.push(Batch {
                    device: self.config.info.name.clone(),
                    channel: channel.info.name.clone(),
                    datapoints: datapoints,
                });
            }
        }
        produced
    }
}

/// A context with the usual maths available under its usual name.
///
/// evalexpr provides these as `math::sqrt` and so on. Someone typing an
/// equation into a box expects `sqrt(dp)`, not `math::sqrt(dp)`, so the plain
/// names are bound here as well. The prefixed forms keep working.
fn fresh_context() -> Context {
    use evalexpr::ContextWithMutableFunctions;
    let mut context = Context::new();
    let unary: [(&str, fn(f64) -> f64); 12] = [
        ("sqrt", f64::sqrt),
        ("abs", f64::abs),
        ("ln", f64::ln),
        ("log10", f64::log10),
        ("exp", f64::exp),
        ("sin", f64::sin),
        ("cos", f64::cos),
        ("tan", f64::tan),
        ("asin", f64::asin),
        ("acos", f64::acos),
        ("atan", f64::atan),
        ("round", f64::round),
    ];
    for (name, function) in unary {
        // set_function only fails on a name that is not a valid identifier, and
        // these are all literals, so there is nothing to handle.
        let _ = context.set_function(
            name.to_string(),
            evalexpr::Function::new(move |argument| {
                let value = argument.as_number()?;
                Ok(evalexpr::Value::from_float(function(value)))
            }),
        );
    }
    context
}

fn channel_equation(config: &CalculatedDevice, name: &str) -> String {
    config
        .channels
        .iter()
        .find(|channel| channel.info.name == name)
        .map(|channel| channel.equation.clone())
        .unwrap_or_default()
}

/// Compile one channel's equation and check it against its declared inputs.
fn compile(channel: &CalculatedChannel) -> Result<Compiled> {
    let tree = evalexpr::build_operator_tree::<evalexpr::DefaultNumericTypes>(&channel.equation)
        .map_err(|error| Error::InvalidEquation {
            channel: channel.info.name.clone(),
            equation: channel.equation.clone(),
            reason: error.to_string(),
        })?;

    // An equation using a name that was never declared would otherwise fail on
    // every sample at run time, having looked fine in the config.
    for used in tree.iter_variable_identifiers() {
        if !channel.inputs.contains_key(used) {
            return Err(Error::UnknownEquationInput {
                channel: channel.info.name.clone(),
                variable: used.to_string(),
                declared: channel.inputs.keys().cloned().collect::<Vec<_>>().join(", "),
            });
        }
    }

    // Building the tree only catches structural faults such as an unmatched
    // parenthesis. An operator missing an argument, a misspelled function or an
    // empty equation all build perfectly well and fail when evaluated, which
    // would mean failing on every sample of a run that had already started.
    // So try it once here, with a stand-in value.
    let mut trial = fresh_context();
    for variable in channel.inputs.keys() {
        use evalexpr::ContextWithMutableVariables;
        trial
            .set_value(variable.into(), evalexpr::Value::from_float(1.0))
            .map_err(|error| Error::InvalidEquation {
                channel: channel.info.name.clone(),
                equation: channel.equation.clone(),
                reason: error.to_string(),
            })?;
    }
    let trial_result = tree
        .eval_with_context(&trial)
        .and_then(|value| value.as_number())
        .map_err(|error| Error::InvalidEquation {
            channel: channel.info.name.clone(),
            equation: channel.equation.clone(),
            reason: error.to_string(),
        })?;
    // Deliberately not checking the trial value is finite. Plenty of sound
    // equations divide by something that happens to be zero at the stand-in
    // value; that is a property of the data, and is caught per sample instead.
    let _ = trial_result;

    let mut inputs = channel.inputs.iter();
    let (variable, source) = match (inputs.next(), inputs.next()) {
        (Some((variable, source)), None) => (variable.clone(), source.clone()),
        (None, _) => {
            return Err(Error::EquationHasNoInput { channel: channel.info.name.clone() })
        }
        // Several inputs need a rule for combining values that never share a
        // timestamp. Refused rather than guessed at.
        (Some(_), Some(_)) => {
            return Err(Error::MultipleEquationInputs {
                channel: channel.info.name.clone(),
                count: channel.inputs.len(),
            })
        }
    };

    Ok(Compiled {
        info: channel.info.clone(),
        tree: tree,
        variable: variable,
        source: source,
    })
}

/// Evaluate one sample, refusing anything that is not a real number.
///
/// A division by zero gives infinity rather than an error, and it would be
/// stored quite happily, so it is caught here: a reading of `inf` looks like
/// data and is not.
fn evaluate(
    tree: &Tree,
    context: &mut Context,
    variable: &str,
    input: f64,
) -> std::result::Result<f64, String> {
    use evalexpr::ContextWithMutableVariables;
    context
        .set_value(variable.into(), evalexpr::Value::from_float(input))
        .map_err(|error| error.to_string())?;
    let value = tree
        .eval_with_context(context)
        .map_err(|error| error.to_string())?;
    let number = value.as_number().map_err(|error| error.to_string())?;
    if !number.is_finite() {
        return Err(format!("gave {} for an input of {}", number, input));
    }
    Ok(number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn channel(name: &str, equation: &str, inputs: &[(&str, &str, &str)]) -> CalculatedChannel {
        CalculatedChannel {
            info: ChannelInfo {
                name: name.to_string(),
                unit: "-".to_string(),
                description: "-".to_string(),
            },
            inputs: inputs
                .iter()
                .map(|(variable, device, source)| {
                    (
                        variable.to_string(),
                        ChannelRef { device: device.to_string(), channel: source.to_string() },
                    )
                })
                .collect(),
            equation: equation.to_string(),
        }
    }

    fn calculator(channels: Vec<CalculatedChannel>) -> Result<Calculator> {
        Calculator::from_config(CalculatedDevice {
            info: DeviceInfo { name: "Derived".to_string(), description: "-".to_string() },
            channels: channels,
        })
    }

    fn batch(device: &str, name: &str, values: &[f64]) -> Batch {
        let start = Utc::now();
        Batch {
            device: device.to_string(),
            channel: name.to_string(),
            datapoints: values
                .iter()
                .enumerate()
                .map(|(index, value)| DataPoint {
                    datetime: start + chrono::Duration::milliseconds(index as i64 * 10),
                    value: *value,
                })
                .collect(),
        }
    }

    fn ignore(_: DeviceEvent) {}

    #[test]
    fn an_equation_is_applied_to_every_sample() {
        let mut calc = calculator(vec![channel(
            "Pressure",
            "(v - 0.5) * 12.5",
            &[("v", "ADC-20", "Input 1")],
        )])
        .unwrap();

        let produced = calc.apply(&batch("ADC-20", "Input 1", &[0.5, 1.0, 2.0]), &mut ignore);
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].device, "Derived");
        assert_eq!(produced[0].channel, "Pressure");
        let values: Vec<f64> = produced[0].datapoints.iter().map(|point| point.value).collect();
        assert_eq!(values, vec![0.0, 6.25, 18.75]);
    }

    /// A calculated value happened when its measurement happened, not when the
    /// arithmetic was done.
    #[test]
    fn output_keeps_the_input_timestamps() {
        let mut calc =
            calculator(vec![channel("Doubled", "v * 2", &[("v", "D", "C")])]).unwrap();
        let source = batch("D", "C", &[1.0, 2.0, 3.0]);
        let produced = calc.apply(&source, &mut ignore);
        let times: Vec<_> = produced[0].datapoints.iter().map(|point| point.datetime).collect();
        let expected: Vec<_> = source.datapoints.iter().map(|point| point.datetime).collect();
        assert_eq!(times, expected);
    }

    #[test]
    fn a_batch_from_another_channel_produces_nothing() {
        let mut calc =
            calculator(vec![channel("Doubled", "v * 2", &[("v", "D", "C")])]).unwrap();
        assert!(calc.apply(&batch("D", "Other", &[1.0]), &mut ignore).is_empty());
        assert!(calc.apply(&batch("Elsewhere", "C", &[1.0]), &mut ignore).is_empty());
    }

    #[test]
    fn one_measured_channel_can_feed_several_calculated_ones() {
        let mut calc = calculator(vec![
            channel("Doubled", "v * 2", &[("v", "D", "C")]),
            channel("Halved", "v / 2", &[("v", "D", "C")]),
        ])
        .unwrap();
        let produced = calc.apply(&batch("D", "C", &[4.0]), &mut ignore);
        assert_eq!(produced.len(), 2);
        assert_eq!(produced[0].datapoints[0].value, 8.0);
        assert_eq!(produced[1].datapoints[0].value, 2.0);
    }

    /// Structural faults are caught when the equation is parsed.
    #[test]
    fn an_unbalanced_equation_is_refused_at_build() {
        let error = calculator(vec![channel("Bad", "v * (2", &[("v", "D", "C")])])
            .err()
            .unwrap();
        assert!(matches!(error, Error::InvalidEquation { .. }));
    }

    /// These all parse. They only fail when evaluated, which without the trial
    /// evaluation would mean failing on every sample of a run already under
    /// way rather than being refused before it started.
    #[test]
    fn equations_that_only_fail_when_run_are_still_refused_at_build() {
        for equation in ["v * * 2", "v +", "v $$ 2", "v 2", ""] {
            let result = calculator(vec![channel("Bad", equation, &[("v", "D", "C")])]);
            assert!(
                matches!(result, Err(Error::InvalidEquation { .. })),
                "{:?} should have been refused",
                equation
            );
        }
    }

    /// An equation may be undefined at the stand-in value and perfectly sound
    /// for real data, so the trial must not require a finite answer.
    #[test]
    fn an_equation_undefined_at_the_trial_value_is_still_accepted() {
        // 1 is the stand-in, so this divides by zero while being built.
        let calc = calculator(vec![channel("Ratio", "1 / (v - 1)", &[("v", "D", "C")])]);
        assert!(calc.is_ok());
    }

    /// An equation naming something never declared would fail on every sample
    /// while looking perfectly reasonable in the config.
    #[test]
    fn an_undeclared_variable_is_refused_at_build() {
        let error = calculator(vec![channel("Bad", "v * offset", &[("v", "D", "C")])])
            .err()
            .unwrap();
        assert!(matches!(error, Error::UnknownEquationInput { .. }));
        assert!(error.to_string().contains("offset"));
    }

    #[test]
    fn an_equation_with_no_input_is_refused() {
        let error = calculator(vec![channel("Constant", "42", &[])]).err().unwrap();
        assert!(matches!(error, Error::EquationHasNoInput { .. }));
    }

    /// Several inputs need a rule for values that never share a timestamp.
    /// Refusing is better than quietly picking one.
    #[test]
    fn several_inputs_are_refused_for_now() {
        let error = calculator(vec![channel(
            "Difference",
            "a - b",
            &[("a", "D", "High"), ("b", "D", "Low")],
        )])
        .err()
        .unwrap();
        assert!(matches!(error, Error::MultipleEquationInputs { .. }));
    }

    /// Infinity is not a reading. It would store perfectly happily and look
    /// like data.
    #[test]
    fn a_non_finite_result_is_skipped_and_reported() {
        let mut calc =
            calculator(vec![channel("Ratio", "1 / v", &[("v", "D", "C")])]).unwrap();
        let mut problems = 0;
        let produced = calc.apply(&batch("D", "C", &[1.0, 0.0, 4.0]), &mut |event| {
            if matches!(event, DeviceEvent::Problem { .. }) {
                problems += 1;
            }
        });
        assert_eq!(problems, 1);
        let values: Vec<f64> = produced[0].datapoints.iter().map(|point| point.value).collect();
        assert_eq!(values, vec![1.0, 0.25]);
    }

    /// evalexpr namespaces these as math::sqrt. Someone typing into a box in a
    /// user interface will write sqrt, so both have to work.
    #[test]
    fn the_usual_maths_works_under_its_usual_name() {
        let mut calc = calculator(vec![
            channel("Root", "sqrt(v)", &[("v", "D", "C")]),
            channel("Namespaced", "math::sqrt(v)", &[("v", "D", "C")]),
            channel("Magnitude", "abs(0 - v)", &[("v", "D", "C")]),
        ])
        .unwrap();
        let produced = calc.apply(&batch("D", "C", &[9.0]), &mut ignore);
        let values: Vec<f64> = produced.iter().map(|b| b.datapoints[0].value).collect();
        assert_eq!(values, vec![3.0, 3.0, 9.0]);
    }

    /// The check a user interface would run behind a text box, on an equation
    /// that did not exist when this was built.
    #[test]
    fn an_equation_can_be_checked_on_its_own() {
        let good = channel("X", "sqrt(v) * 2", &[("v", "D", "C")]);
        assert!(good.validate().is_ok());

        let bad = channel("X", "v * (2", &[("v", "D", "C")]);
        assert!(bad.validate().is_err());
    }

    #[test]
    fn sources_lists_what_the_equations_read() {
        let calc = calculator(vec![
            channel("A", "v * 2", &[("v", "Rig", "Volts")]),
            channel("B", "v + 1", &[("v", "Other", "Amps")]),
        ])
        .unwrap();
        let sources = calc.sources();
        assert!(sources.contains(&ChannelRef {
            device: "Rig".to_string(),
            channel: "Volts".to_string()
        }));
        assert!(sources.contains(&ChannelRef {
            device: "Other".to_string(),
            channel: "Amps".to_string()
        }));
    }
}
