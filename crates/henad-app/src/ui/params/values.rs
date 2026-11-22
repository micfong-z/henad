//! Widgets and text of one parameter value: the slider, checkbox or list of each kind, a fraction shown as a
//! percentage, and the number a sheet parameter holds.

use henad_core::explore::value::format_value;
use henad_core::params::{ParamDescriptor, ParamFormat, ParamKind, ParamValue};

/// Draws the widget of parameter `descriptor` over `value`, labelled `text` and described by `hint`, and returns
/// whether the user changed the value. A value of another kind than `descriptor` declares gets no widget.
pub(super) fn value_widget(
    ui: &mut egui::Ui,
    descriptor: &ParamDescriptor,
    value: &mut ParamValue,
    text: egui::RichText,
    hint: Option<&str>,
) -> bool {
    match (descriptor.kind(), value) {
        (ParamKind::F32 { min, max, step, .. }, ParamValue::F32(number)) => {
            let before = *number;
            let mut slider = f32_slider(number, *min, *max, parameter_slider_step(descriptor, *step)).text(text);
            if descriptor.format() == ParamFormat::Percent {
                slider = as_percent(slider, *step);
            }
            with_hint(ui.add(slider), hint);
            *number != before
        }
        (ParamKind::U32 { min, max, .. }, ParamValue::U32(number)) => {
            with_hint(ui.add(egui::Slider::new(number, *min..=*max).text(text)), hint).changed()
        }
        (ParamKind::Bool { .. }, ParamValue::Bool(flag)) => with_hint(ui.checkbox(flag, text), hint).changed(),
        (ParamKind::Choice { options, .. }, ParamValue::Choice(choice)) => {
            let mut changed = false;
            let combo = egui::ComboBox::from_label(text)
                .selected_text(options.get(*choice).unwrap_or("?"))
                .show_ui(ui, |ui| {
                    for (index, option) in options.iter().enumerate() {
                        changed |= ui.selectable_value(choice, index, option).changed();
                    }
                });
            with_hint(combo.response, hint);
            changed
        }
        _ => false,
    }
}

/// Returns a slider over `value` from `min` to `max`, snapping to `step`.
///
/// Note that the slider snaps `value` only when the user edits it. Drawing it leaves a value set by a spec or an
/// opened run as it is.
fn f32_slider(value: &mut f32, min: f32, max: f32, step: Option<f32>) -> egui::Slider<'_> {
    let range = decimal_value(min)..=decimal_value(max);
    let slider = egui::Slider::from_get_set(range, move |edited| {
        if let Some(edited) = edited {
            *value = edited as f32;
        }
        f64::from(*value)
    })
    .clamping(egui::SliderClamping::Edits);
    match step {
        Some(step) => slider.step_by(decimal_value(step)),
        None => slider,
    }
}

/// Returns the step of the slider of parameter `descriptor`, whose kind declares `step`.
///
/// An `f32` marked integer steps by its own step rounded to a whole number, or by 1, so the slider holds whole numbers
/// alone.
fn parameter_slider_step(descriptor: &ParamDescriptor, step: Option<f32>) -> Option<f32> {
    if descriptor.is_integer() {
        Some(step.map_or(1.0, |step| step.round().max(1.0)))
    } else {
        step
    }
}

/// Returns `value` widened to `f64` through its shortest decimal form.
///
/// A slider snaps to multiples of its step in `f64`. A step of `0.01_f32` widened in binary would snap 0.05 to the
/// `f32` below it.
fn decimal_value(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

/// Returns `value` as the number a sheet parameter holds, `None` for a kind a sheet parameter cannot hold.
///
/// A float is widened through [`decimal_value`], so a kept or shown 0.3 stays 0.3.
pub(crate) fn sheet_number(value: &ParamValue) -> Option<f64> {
    match value {
        ParamValue::F32(number) => Some(decimal_value(*number)),
        ParamValue::U32(number) => Some(f64::from(*number)),
        _ => None,
    }
}

/// Returns `value` as the Parameters panel shows it, a fraction as a percentage.
pub(crate) fn display_value(descriptor: &ParamDescriptor, value: &ParamValue) -> String {
    match (descriptor.kind(), value) {
        (ParamKind::F32 { step, .. }, ParamValue::F32(number)) if descriptor.format() == ParamFormat::Percent => {
            percent_text(f64::from(*number), percent_decimals(*step))
        }
        _ => format_value(descriptor.kind(), value),
    }
}

/// Returns the fraction `fraction` as a percentage with `decimals` decimals.
fn percent_text(fraction: f64, decimals: usize) -> String {
    format!("{:.decimals$}%", fraction * 100.0)
}

/// Returns `slider` showing and reading its fraction as a percentage.
fn as_percent(slider: egui::Slider<'_>, step: Option<f32>) -> egui::Slider<'_> {
    let decimals = percent_decimals(step);
    slider
        .custom_formatter(move |value, _| percent_text(value, decimals))
        .custom_parser(|text| {
            let number = text.trim().trim_end_matches('%').trim_end();
            number.parse::<f64>().ok().map(|percent| percent / 100.0)
        })
}

/// Returns the smallest number of decimals that shows each step as a distinct percentage.
fn percent_decimals(step: Option<f32>) -> usize {
    let Some(step) = step else {
        return 1;
    };
    let percent = f64::from(step) * 100.0;
    (0..4)
        .find(|&places| {
            let scaled = percent * 10f64.powi(places);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .map_or(4, |places| places as usize)
}

fn with_hint(response: egui::Response, hint: Option<&str>) -> egui::Response {
    match hint {
        Some(hint) => response.on_hover_text(hint),
        None => response,
    }
}

#[cfg(test)]
mod tests {
    use henad_core::explore::value::{check_param, parse_value};
    use henad_core::helpers::{f32_param, u32_param};
    use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
    use henad_models::example_models;

    use super::{decimal_value, display_value, f32_slider, percent_decimals, value_widget};

    /// Returns the input of a frame in which each of `keys` is pressed.
    fn key_presses(keys: &[egui::Key]) -> egui::RawInput {
        let events = keys
            .iter()
            .map(|&key| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .collect();
        egui::RawInput {
            events,
            ..egui::RawInput::default()
        }
    }

    /// Draws the slider of an F32 parameter of `kind` over `value` for one frame of `context`, focused and pressed
    /// with `keys`, and returns whether it reports a change.
    fn draw_slider(context: &egui::Context, kind: &ParamKind, value: &mut f32, keys: &[egui::Key]) -> bool {
        let ParamKind::F32 { min, max, step, .. } = *kind else {
            panic!("an F32 parameter");
        };
        let input = key_presses(keys);
        let mut changed = false;
        let output = context.run_ui(input, |ui| {
            let response = ui.add(f32_slider(value, min, max, step));
            // A request resets the focus lock that keeps the arrow keys on the slider.
            if !response.has_focus() {
                response.request_focus();
            }
            changed |= response.changed();
        });
        output.drop_without_applying_deltas();
        changed
    }

    #[test]
    fn a_slider_leaves_a_value_alone_until_it_is_edited() {
        let models = example_models();
        let sliders = models.iter().flat_map(|entry| {
            entry
                .param_descriptors()
                .iter()
                .map(move |descriptor| (entry.id(), descriptor))
        });
        let mut checked = 0;
        for (model, descriptor) in sliders {
            let ParamKind::F32 {
                default,
                min,
                max,
                step,
            } = *descriptor.kind()
            else {
                continue;
            };
            let context = egui::Context::default();
            let mut value = default;
            for _ in 0..2 {
                let changed = draw_slider(&context, descriptor.kind(), &mut value, &[]);
                assert!(!changed, "{model} {}: drawing reports a change", descriptor.id());
                assert_eq!(value.to_bits(), default.to_bits(), "{model} {}: drawn", descriptor.id());
            }
            let Some(step) = step else {
                continue;
            };
            // A step there and back lands on the value it left, when that value is on the slider's grid.
            let offset = (decimal_value(default) - decimal_value(min)) / decimal_value(step);
            if (offset - offset.round()).abs() > 1e-9 {
                continue;
            }
            let (there, back) = if default + step <= max {
                (egui::Key::ArrowRight, egui::Key::ArrowLeft)
            } else {
                (egui::Key::ArrowLeft, egui::Key::ArrowRight)
            };
            assert!(draw_slider(&context, descriptor.kind(), &mut value, &[there]));
            assert_ne!(value, default, "{model} {}: the key moves the slider", descriptor.id());
            assert!(draw_slider(&context, descriptor.kind(), &mut value, &[back]));
            assert_eq!(
                value.to_bits(),
                default.to_bits(),
                "{model} {}: {value} after a step there and back",
                descriptor.id()
            );
            checked += 1;
        }
        assert!(checked > 0, "no model declares a stepped F32 parameter");
    }

    /// Draws the widget of `descriptor` over `value` for one frame of `context`, with `keys` pressed, and returns
    /// whether it reports a change.
    fn draw_value(
        context: &egui::Context,
        descriptor: &ParamDescriptor,
        value: &mut ParamValue,
        keys: &[egui::Key],
    ) -> bool {
        let input = key_presses(keys);
        let mut changed = false;
        let output = context.run_ui(input, |ui| {
            changed = value_widget(ui, descriptor, value, egui::RichText::new(descriptor.label()), None);
        });
        output.drop_without_applying_deltas();
        changed
    }

    /// A whole number past `i32::MAX`, in a range that starts below it, steps up from where it is. Through an `i32` the
    /// range and the value would wrap negative.
    #[test]
    fn a_whole_number_slider_holds_a_value_past_the_largest_i32() {
        let start = 3_000_000_000;
        let descriptor = u32_param("count", "Count", start, 0, u32::MAX);
        let context = egui::Context::default();
        let mut value = ParamValue::U32(start);

        assert!(
            !draw_value(&context, &descriptor, &mut value, &[]),
            "drawing reports a change"
        );
        assert_eq!(value, ParamValue::U32(start), "drawn");
        // Tab moves the focus to the slider, and the arrow key then steps it.
        draw_value(&context, &descriptor, &mut value, &[egui::Key::Tab]);
        assert!(draw_value(&context, &descriptor, &mut value, &[egui::Key::ArrowRight]));
        assert!(
            matches!(value, ParamValue::U32(stepped) if stepped > start),
            "{value:?} after a step up from {start}"
        );
    }

    /// The slider of an `f32` marked integer with no step of its own steps by 1, and holds whole numbers alone.
    #[test]
    fn an_integer_slider_steps_by_whole_numbers() {
        let descriptor = f32_param("offset", "Offset", 0.0, -3.0, 3.0, None).integer();
        let context = egui::Context::default();
        let mut value = ParamValue::F32(0.0);
        draw_value(&context, &descriptor, &mut value, &[egui::Key::Tab]);
        assert!(draw_value(&context, &descriptor, &mut value, &[egui::Key::ArrowRight]));
        assert_eq!(value, ParamValue::F32(1.0));
        assert_eq!(check_param(&descriptor, &value), Ok(()));
    }

    #[test]
    fn a_stepped_value_is_the_one_a_spec_writes() {
        let models = example_models();
        let sir = models.get("sir").expect("SIR is registered");
        let descriptor = sir
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id() == "recovery_rate")
            .expect("SIR declares a recovery rate");
        let context = egui::Context::default();
        let mut value = 0.04;
        for keys in [&[][..], &[], &[egui::Key::ArrowRight]] {
            draw_slider(&context, descriptor.kind(), &mut value, keys);
        }
        assert_eq!(
            Ok(ParamValue::F32(value)),
            parse_value(descriptor.kind(), "0.05"),
            "0.04 and one step is 0.05"
        );
    }

    #[test]
    fn a_percentage_shows_as_the_panel_writes_it() {
        let models = example_models();
        let sir = models.get("sir").expect("SIR is registered");
        let percent = sir
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id() == "initial_infected_pct")
            .expect("SIR declares its initially infected share");
        let rate = sir
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id() == "infection_rate")
            .expect("SIR declares an infection rate");
        assert_eq!(display_value(percent, &ParamValue::F32(0.01)), "1.0%");
        assert_eq!(display_value(rate, &ParamValue::F32(0.3)), "0.3");
    }

    #[test]
    fn a_percentage_shows_as_many_decimals_as_its_step_needs() {
        assert_eq!(percent_decimals(Some(0.01)), 0, "1% steps");
        assert_eq!(percent_decimals(Some(0.001)), 1, "0.1% steps");
        assert_eq!(percent_decimals(Some(0.005)), 1, "0.5% steps");
        assert_eq!(percent_decimals(Some(0.0025)), 2, "0.25% steps");
        assert_eq!(percent_decimals(None), 1, "no step");
    }
}
