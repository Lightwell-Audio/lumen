use eframe::egui::{Pos2, Rect, pos2, vec2};
use lumen_devices::eq;

use super::bands::{Band, band, colour};
use super::parts::{
    FINE, KEY_DB, aim, at, drag, first, nudge, pill_words, reset_all, reset_point, wheel, words,
};
use super::picture::{
    bracket_ends, chip_rect, curve_line, fill_points, mist_points, node_centre, stem_rect,
};
use super::plot::Plot;
use super::response::{Shape, response};
use super::{DRAWING, READS, WHOLE, pill};
use crate::chain::face::Values;
use crate::chain::strip::Slot;

fn slots(set: &[(u32, f32)]) -> Vec<Slot> {
    READS
        .iter()
        .chain(DRAWING.controls.iter().map(|c| &c.param))
        .map(|id| {
            let info = lumen_devices::params::find(eq::TABLE, *id).unwrap();
            let plain = set
                .iter()
                .find(|(known, _)| known == id)
                .map_or(info.default, |(_, value)| *value);
            Slot {
                id: *id,
                name: String::new(),
                text: String::new(),
                value: f64::from(info.normalised(plain)),
                default: f64::from(info.default),
                steps: info.steps,
                read_only: false,
                automatable: true,
                choices: Vec::new(),
                over: false,
                mapped: None,
                waiting: false,
            }
        })
        .collect()
}

fn values(slots: &[Slot]) -> Values<'_> {
    Values {
        slots,
        table: eq::TABLE,
        rate: 48_000,
    }
}

fn screen() -> Rect {
    Rect::from_min_size(pos2(14.0, 60.0), vec2(352.0, 176.0))
}

#[test]
fn every_parameter_the_parts_read_is_in_the_table_once() {
    for id in READS {
        assert!(
            lumen_devices::params::find(eq::TABLE, id).is_some(),
            "{id} is not in the table"
        );
    }
    let mut seen = READS.to_vec();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), READS.len());
    for control in DRAWING.controls {
        assert!(lumen_devices::params::find(eq::TABLE, control.param).is_some());
    }
}

#[test]
fn a_point_says_its_band_in_words() {
    let held = slots(&[(eq::freq_of(3), 1_200.0), (eq::gain_of(3), 3.0)]);
    let v = values(&held);
    let said = words(&v, 2);
    assert!(
        said.starts_with("Band 3, bell, 1.20 kHz, +3.0 dB"),
        "{said}"
    );
    assert!(said.contains("Q 1.00"), "{said}");
    let off = words(&v, 0);
    assert!(off.contains("low cut"), "{off}");
    assert!(off.contains("24/oct"), "{off}");
    assert_eq!(pill_words(&v, 2), "Band 3 (bell)");
}

#[test]
fn the_chip_names_what_the_band_is() {
    let held = slots(&[
        (eq::freq_of(2), 180.0),
        (eq::gain_of(2), 5.0),
        (eq::freq_of(1), 40.0),
    ]);
    let v = values(&held);
    assert_eq!(band(&v, 1).chip(), "180 Hz +5.0");
    assert_eq!(band(&v, 0).chip(), "40.0 Hz 24/oct");
    let notch = slots(&[
        (eq::type_of(4), 3.0),
        (eq::freq_of(4), 2_500.0),
        (eq::q_of(4), 4.0),
    ]);
    assert_eq!(band(&values(&notch), 3).chip(), "2.50 kHz Q 4.00");
}

#[test]
fn a_drag_reads_the_axis_the_curve_is_drawn_on() {
    let rect = screen();
    let held = slots(&[(eq::freq_of(3), 1_000.0), (eq::gain_of(3), 6.0)]);
    let v = values(&held);
    let place = at(&v, 2, rect).unwrap();
    let written = drag(&v, 2, place, rect);
    let get = |id: u32| written.iter().find(|(k, _)| *k == id).map(|(_, x)| *x);
    let hz = v.plain_at(eq::freq_of(3), get(eq::freq_of(3)).unwrap() as f32);
    let db = v.plain_at(eq::gain_of(3), get(eq::gain_of(3)).unwrap() as f32);
    assert!((hz - 1_000.0).abs() < 15.0, "{hz}");
    assert!((db - 6.0).abs() < 0.2, "{db}");
}

#[test]
fn a_drag_turns_an_off_band_on_and_a_cut_moves_only_its_frequency() {
    let rect = screen();
    let held = slots(&[(eq::on_of(3), 0.0)]);
    let v = values(&held);
    let written = drag(&v, 2, pos2(100.0, 100.0), rect);
    assert!(written.contains(&(eq::on_of(3), 1.0)));
    let on = slots(&[(eq::on_of(1), 1.0)]);
    let cut = drag(&values(&on), 0, pos2(100.0, 100.0), rect);
    assert!(cut.iter().all(|(id, _)| *id == eq::freq_of(1)), "{cut:?}");
}

#[test]
fn the_dials_aim_at_what_the_picked_band_has() {
    let held = slots(&[]);
    let v = values(&held);
    let aimed = |band: usize, control: usize| aim(&v, band, control).map(|a| (a.param, a.name));
    assert_eq!(aimed(1, 0), Some((eq::freq_of(2), "Freq")));
    assert_eq!(aimed(1, 1), Some((eq::gain_of(2), "Gain")));
    assert_eq!(aimed(1, 2), Some((eq::q_of(2), "Q")));
    assert_eq!(aimed(0, 1), Some((eq::gain_of(1), "Slope")));
    assert_eq!(aimed(0, 2), None);
    let notch = slots(&[(eq::type_of(4), 3.0)]);
    let shelf = slots(&[(eq::type_of(5), 1.0)]);
    assert_eq!(aim(&values(&notch), 3, 1).map(|a| a.name), None);
    assert_eq!(aim(&values(&notch), 3, 2).map(|a| a.name), Some("Q"));
    assert_eq!(aim(&values(&shelf), 4, 1).map(|a| a.name), Some("Gain"));
    assert_eq!(aim(&values(&shelf), 4, 2).map(|a| a.name), None);
}

#[test]
fn the_wheel_is_q_for_a_bell_and_slope_for_a_cut() {
    let held = slots(&[]);
    let v = values(&held);
    let (id, norm) = wheel(&v, 1, 1.0, false).unwrap();
    assert_eq!(id, eq::q_of(2));
    assert!(v.plain_at(id, norm as f32) > v.plain(id));
    let (id, norm) = wheel(&v, 0, 1.0, false).unwrap();
    assert_eq!(id, eq::gain_of(1));
    assert!((v.plain_at(id, norm as f32) - 36.0).abs() < 0.01);
    let notch = slots(&[(eq::type_of(4), 3.0)]);
    assert!(wheel(&values(&notch), 3, 1.0, false).is_some());
    let shelf = slots(&[(eq::type_of(5), 1.0)]);
    assert!(wheel(&values(&shelf), 4, 1.0, false).is_none());
}

#[test]
fn a_nudge_moves_by_a_step_and_shift_by_a_fifth_of_it() {
    let held = slots(&[(eq::freq_of(2), 1_000.0), (eq::gain_of(2), 0.0)]);
    let v = values(&held);
    let plain = |written: &[(u32, f64)], id: u32| {
        v.plain_at(id, written.iter().find(|(k, _)| *k == id).unwrap().1 as f32)
    };
    let coarse = nudge(&v, 1, vec2(0.0, 1.0), false);
    let fine = nudge(&v, 1, vec2(0.0, 1.0), true);
    assert!((plain(&coarse, eq::gain_of(2)) - KEY_DB).abs() < 0.01);
    assert!((plain(&fine, eq::gain_of(2)) - KEY_DB * FINE).abs() < 0.01);
    let right = nudge(&v, 1, vec2(1.0, 0.0), false);
    assert!(plain(&right, eq::freq_of(2)) > 1_000.0);
}

#[test]
fn a_reset_writes_only_what_the_band_has() {
    let held = slots(&[(eq::gain_of(2), 7.0), (eq::gain_of(1), 48.0)]);
    let v = values(&held);
    let ids = |written: Vec<(u32, f64)>| written.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    assert_eq!(ids(reset_point(&v, 1)), [eq::gain_of(2)]);
    assert_eq!(ids(reset_point(&v, 0)), [eq::freq_of(1)]);
    assert_eq!(
        ids(reset_all(&v, 1)),
        [eq::freq_of(2), eq::gain_of(2), eq::q_of(2)]
    );
    assert_eq!(ids(reset_all(&v, 0)), [eq::freq_of(1), eq::gain_of(1)]);
}

#[test]
fn the_pills_sit_in_two_rows_of_four_inside_the_card() {
    let card = Rect::from_min_size(pos2(0.0, 0.0), WHOLE);
    let mut seen: Vec<Rect> = Vec::new();
    for index in 0..eq::BANDS {
        let rect = pill(card, index);
        assert!(card.contains_rect(rect), "{index}: {rect:?}");
        assert!(seen.iter().all(|other| !other.intersects(rect)));
        seen.push(rect);
    }
    assert_eq!(seen[0].top(), seen[3].top());
    assert!(seen[4].top() > seen[0].bottom());
}

#[test]
fn eight_bands_have_eight_colours_and_the_first_band_is_the_first_on() {
    let colours: std::collections::HashSet<_> = (0..eq::BANDS).map(colour).collect();
    assert_eq!(colours.len(), eq::BANDS);
    let held = slots(&[
        (eq::on_of(1), 0.0),
        (eq::on_of(2), 0.0),
        (eq::on_of(3), 1.0),
    ]);
    assert_eq!(first(&values(&held)), 2);
    let none = slots(&[
        (eq::on_of(1), 0.0),
        (eq::on_of(2), 0.0),
        (eq::on_of(3), 0.0),
        (eq::on_of(4), 0.0),
        (eq::on_of(5), 0.0),
        (eq::on_of(6), 0.0),
        (eq::on_of(7), 0.0),
        (eq::on_of(8), 0.0),
    ]);
    assert_eq!(first(&values(&none)), 0);
}

fn interpolate(line: &[Pos2], x: f32) -> f32 {
    let at = line.partition_point(|point| point.x < x);
    if at == 0 {
        return line[0].y;
    }
    if at >= line.len() {
        return line[line.len() - 1].y;
    }
    let (a, b) = (line[at - 1], line[at]);
    let t = (x - a.x) / (b.x - a.x).max(1e-6);
    a.y + (b.y - a.y) * t
}

fn shaped() -> Vec<Slot> {
    slots(&[
        (eq::on_of(1), 1.0),
        (eq::freq_of(1), 40.0),
        (eq::on_of(2), 1.0),
        (eq::freq_of(2), 180.0),
        (eq::gain_of(2), 5.0),
        (eq::on_of(3), 1.0),
        (eq::freq_of(3), 900.0),
        (eq::gain_of(3), -7.0),
        (eq::q_of(3), 3.0),
        (eq::on_of(4), 1.0),
        (eq::freq_of(4), 1_100.0),
        (eq::gain_of(4), 4.0),
        (eq::q_of(4), 0.8),
        (eq::on_of(5), 1.0),
        (eq::freq_of(5), 5_000.0),
        (eq::gain_of(5), 3.0),
    ])
}

fn each_size() -> [Rect; 3] {
    [
        Rect::from_min_size(pos2(14.0, 60.0), vec2(352.0, 176.0)),
        Rect::from_min_size(pos2(14.0, 60.0), vec2(300.0, 102.0)),
        Rect::from_min_size(pos2(14.0, 60.0), vec2(220.0, 22.0)),
    ]
}

#[test]
fn the_curve_passes_through_every_drawn_node() {
    let held = shaped();
    let v = values(&held);
    let response = response(&v);
    for rect in each_size() {
        let plot = Plot::of(rect);
        for ppp in [1.0, 1.5, 2.0] {
            let line = curve_line(&plot, &response, ppp);
            for index in 0..eq::BANDS {
                let band = band(&v, index);
                if !band.on {
                    continue;
                }
                let centre = node_centre(&plot, &response, &band);
                let y = interpolate(&line, centre.x);
                assert!(
                    (y - centre.y).abs() < 0.06,
                    "band {} at {rect:?} x{ppp}: node {} curve {y}",
                    index + 1,
                    centre.y
                );
            }
        }
    }
}

#[test]
fn the_hit_point_is_the_drawn_node() {
    let held = shaped();
    let v = values(&held);
    let response = response(&v);
    for rect in each_size() {
        let plot = Plot::of(rect);
        for index in 0..eq::BANDS {
            let drawn = node_centre(&plot, &response, &band(&v, index));
            assert_eq!(at(&v, index, rect), Some(drawn));
        }
    }
}

#[test]
fn a_ray_starts_at_its_node_and_ends_on_the_zero_line() {
    let held = shaped();
    let v = values(&held);
    let response = response(&v);
    let plot = Plot::of(each_size()[0]);
    for index in 0..eq::BANDS {
        let band = band(&v, index);
        if !band.on || !band.has_stem() {
            continue;
        }
        let centre = node_centre(&plot, &response, &band);
        let rect = stem_rect(&plot, centre, centre.x - 0.5);
        assert!(rect.left() <= centre.x && centre.x <= rect.right());
        let (near, far) = if centre.y < plot.zero() {
            (rect.top(), rect.bottom())
        } else {
            (rect.bottom(), rect.top())
        };
        assert!((near - centre.y).abs() < 1e-4, "band {}", index + 1);
        assert!((far - plot.zero()).abs() < 1e-4, "band {}", index + 1);
    }
}

#[test]
fn a_band_fill_follows_the_bands_own_curve_and_meets_the_node() {
    let held = shaped();
    let v = values(&held);
    let plot = Plot::of(each_size()[0]);
    for index in 1..5 {
        let band = band(&v, index);
        let own = Shape::of(&band.setting(), 48_000);
        let points = fill_points(&plot, &own, band.freq);
        assert!(points.len() < 400, "{}", points.len());
        let x = plot.x_of(band.freq);
        let want = plot.y_of(own.db_at_hz(band.freq));
        assert!(
            (interpolate(&points, x) - want).abs() < 0.06,
            "band {}",
            index + 1
        );
        for point in &points {
            assert!(plot.area.expand(0.01).contains(*point));
        }
    }
}

#[test]
fn the_bracket_is_centred_on_the_node_and_stays_in_the_plot() {
    let held = shaped();
    let v = values(&held);
    let plot = Plot::of(each_size()[0]);
    for index in 1..5 {
        let band: Band = band(&v, index);
        let (left, right) = bracket_ends(&plot, &band).unwrap();
        let x = plot.x_of(band.freq);
        assert!(left < x && x < right, "band {}", index + 1);
        assert!(left >= plot.area.left() && right <= plot.area.right());
    }
    assert!(bracket_ends(&plot, &band(&v, 0)).is_none());
}

#[test]
fn the_analyser_is_thinned_to_the_pixels_and_stays_in_the_plot() {
    let plot = Plot::of(each_size()[0]);
    let spectrum: Vec<f32> = (0..479).map(|i| -90.0 + (i % 80) as f32).collect();
    let points = mist_points(&plot, &spectrum);
    assert!(points.len() < spectrum.len());
    assert!(points.len() > 100);
    assert!((points[0].x - plot.area.left()).abs() < 1e-3);
    assert!((points[points.len() - 1].x - plot.area.right()).abs() < 1e-3);
    for point in &points {
        assert!(plot.area.expand(0.01).contains(*point), "{point:?}");
    }
}

#[test]
fn the_plot_maps_both_ways_and_labels_only_when_there_is_room() {
    let [whole, short, compact] = each_size();
    let plot = Plot::of(whole);
    assert!(plot.labelled && plot.chip);
    for db in [-12.0, -3.5, 0.0, 6.0, 12.0] {
        assert!((plot.db_of_y(plot.y_of(db)) - db).abs() < 1e-3);
    }
    for hz in [30.0, 100.0, 1_000.0, 10_000.0] {
        let share = plot.share_of_x(plot.x_of(hz));
        assert!((eq::axis_hz(share) - hz).abs() / hz < 1e-3);
    }
    assert!(plot.area.contains(plot.place(1_000.0, 0.0)));
    let short = Plot::of(short);
    assert!(short.labelled && !short.chip);
    let compact = Plot::of(compact);
    assert!(!compact.labelled && !compact.chip);
    assert!(compact.area.height() > 10.0);
}

#[test]
fn the_chip_stays_inside_the_screen_and_off_its_node() {
    let rect = each_size()[0];
    let plot = Plot::of(rect);
    for hz in [20.0, 90.0, 1_000.0, 9_000.0, 20_000.0] {
        for db in [-14.0, 0.0, 14.0] {
            let centre = plot.place(hz, db);
            let chip = chip_rect(&plot, rect, centre, 96.0);
            assert!(rect.contains_rect(chip), "{hz} {db}: {chip:?}");
            assert!(!chip.contains(centre), "{hz} {db}: {chip:?}");
        }
    }
}

#[test]
fn a_drag_keeps_the_node_under_the_pointer_with_other_bands_in_play() {
    let rect = each_size()[0];
    let held = shaped();
    let v = values(&held);
    let plot = Plot::of(rect);
    let pointer = plot.place(1_000.0, 8.0);
    let written = drag(&v, 2, pointer, rect);
    let mut again = held.clone();
    for slot in &mut again {
        if let Some((_, norm)) = written.iter().find(|(id, _)| *id == slot.id) {
            slot.value = *norm;
        }
    }
    let after = values(&again);
    let landed = at(&after, 2, rect).unwrap();
    assert!((landed.x - pointer.x).abs() < 1.5, "{landed:?} {pointer:?}");
    assert!((landed.y - pointer.y).abs() < 0.3, "{landed:?} {pointer:?}");
}
