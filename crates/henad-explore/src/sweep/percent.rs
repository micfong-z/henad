//! Values of a spec written in percent, rewritten as the fractions that `runs.csv` holds.

use henad_core::explore::factor::{FactorSpec, FactorTarget, LevelSpec};
use henad_core::explore::spec::SweepSpec;
use henad_core::explore::value::{format_value, parse_param};
use henad_core::params::{ParamDescriptor, ParamFormat, ParamKind};

/// Returns `spec` with each value written in percent, of an `f32` parameter in the [`ParamFormat::Percent`] format,
/// written as its fraction, and each range in percent of such a parameter as the range of its fractions.
///
/// A manifest then records the fractions that `runs.csv` holds, and a search hashes `1%` as it hashes `0.01`. A value
/// that does not read, or a `%` on any other parameter, is kept as written for the plan to reject. Note that a design
/// table keeps its text.
pub(crate) fn with_fractions(mut spec: SweepSpec, params: &[ParamDescriptor]) -> SweepSpec {
    let percentage = |id: &str| {
        params
            .iter()
            .find(|descriptor| descriptor.id() == id)
            .filter(|descriptor| {
                descriptor.format() == ParamFormat::Percent && matches!(descriptor.kind(), ParamKind::F32 { .. })
            })
    };
    let fraction = |descriptor: &ParamDescriptor, text: &str| -> Option<String> {
        let text = text.trim();
        if !text.ends_with('%') {
            return None;
        }
        let value = parse_param(descriptor, text).ok()?;
        Some(format_value(descriptor.kind(), &value))
    };
    let factor = |factor: &FactorSpec| {
        let FactorTarget::Param(id) = &factor.target else {
            return factor.clone();
        };
        let Some(descriptor) = percentage(id) else {
            return factor.clone();
        };
        let levels = match &factor.levels {
            LevelSpec::Values(values) => LevelSpec::Values(
                values
                    .iter()
                    .map(|text| fraction(descriptor, text).unwrap_or_else(|| text.clone()))
                    .collect(),
            ),
            levels => levels.to_fractions(),
        };
        FactorSpec::param(id.clone(), levels)
    };
    for (id, text) in &mut spec.fixed {
        if let Some(fraction) = percentage(id).and_then(|descriptor| fraction(descriptor, text)) {
            *text = fraction;
        }
    }
    for block in &mut spec.blocks {
        block.factors = block.factors.iter().map(factor).collect();
    }
    if let Some(search) = &mut spec.search {
        search.space = search.space.iter().map(factor).collect();
    }
    spec
}

#[cfg(test)]
mod tests {
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};
    use henad_core::helpers::{f32_param, u32_param};
    use henad_core::params::ParamDescriptor;

    use super::with_fractions;

    fn params() -> Vec<ParamDescriptor> {
        vec![
            f32_param("share", "Share", 0.01, 0.0, 1.0, None).percent(),
            f32_param("rate", "Rate", 0.3, 0.0, 5.0, None),
            u32_param("size", "Size", 8, 1, 64),
        ]
    }

    fn values(raw: &[&str]) -> LevelSpec {
        LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
    }

    /// A spec over `factors` with `fixed` values, in one factorial block.
    fn spec(fixed: &[(&str, &str)], factors: Vec<FactorSpec>) -> SweepSpec {
        let mut spec = SweepSpec::new("model");
        spec.fixed = fixed
            .iter()
            .map(|&(id, text)| (id.to_owned(), text.to_owned()))
            .collect();
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors,
            design_seed: None,
        }];
        spec
    }

    /// Percent text of a percentage becomes its fraction, in fixed values, listed values and ranges, and the plan of
    /// the rewritten spec is the plan of the spec as written.
    #[test]
    fn percent_text_of_a_percentage_becomes_its_fraction() {
        let written = spec(
            &[("share", "0.3%")],
            vec![FactorSpec::param("share", values(&["1%", " 2.5 % ", "0.05"]))],
        );
        let rewritten = with_fractions(written, &params());
        assert_eq!(rewritten.fixed, [("share".to_owned(), "0.003".to_owned())]);
        assert_eq!(
            rewritten.blocks[0].factors[0].levels,
            values(&["0.01", "0.025", "0.05"])
        );

        let range = spec(
            &[],
            vec![FactorSpec::param(
                "share",
                LevelSpec::PercentRange {
                    min: 1.0,
                    max: 5.0,
                    step: Some(1.5),
                },
            )],
        );
        assert_eq!(
            with_fractions(range, &params()).blocks[0].factors[0].levels,
            LevelSpec::Range {
                min: 0.01,
                max: 0.05,
                step: Some(0.015)
            }
        );
    }

    /// A `%` on another parameter, or text that does not read, is kept for the plan to reject.
    #[test]
    fn other_text_is_kept_as_written() {
        let written = spec(
            &[("rate", "10%"), ("size", "50%")],
            vec![
                FactorSpec::param("share", values(&["x%", "0.02"])),
                FactorSpec::param(
                    "rate",
                    LevelSpec::PercentRange {
                        min: 1.0,
                        max: 5.0,
                        step: None,
                    },
                ),
            ],
        );
        assert_eq!(with_fractions(written.clone(), &params()), written);
    }
}
