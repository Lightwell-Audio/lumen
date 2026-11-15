use eframe::egui::{Color32, Context, LayerId, RawInput, Shape, pos2};

use super::*;

fn painted(draw: impl Fn(&Painter)) -> Vec<Shape> {
    let ctx = Context::default();
    let mut out = ctx.run_ui(RawInput::default(), |ui| {
        draw(&ui.ctx().layer_painter(LayerId::background()));
    });
    out.textures_delta.clear();
    out.shapes.into_iter().map(|s| s.shape).collect()
}

fn meshes(shapes: &[Shape]) -> Vec<&Mesh> {
    shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Mesh(mesh) => Some(&**mesh),
            _ => None,
        })
        .collect()
}

#[test]
fn a_glow_is_one_small_mesh_that_fades_to_nothing_at_its_edge() {
    let shapes = painted(|p| glow_ladder(p, pos2(40.0, 40.0), 20.0, Color32::RED, 1.0, false));
    let found = meshes(&shapes);
    assert_eq!(found.len(), 1);
    let mesh = found[0];
    assert_eq!(mesh.vertices.len(), 1 + (ladder::STOPS.len() - 1) * ladder::SIDES);
    let centre = mesh.vertices[0].color;
    let edge = mesh.vertices.last().unwrap().color;
    assert!(centre.a() > edge.a());
    assert_eq!(edge.a(), 0);
    let ring = |k: usize| mesh.vertices[1 + k * ladder::SIDES].color.a();
    assert!(ring(0) > ring(1) && ring(1) > ring(2), "stops fall in order");
}

#[test]
fn an_additive_glow_has_no_alpha_so_it_adds_light() {
    let shapes = painted(|p| glow_ladder(p, pos2(40.0, 40.0), 20.0, Color32::RED, 1.0, true));
    let mesh = meshes(&shapes)[0];
    assert!(mesh.vertices.iter().all(|v| v.color.a() == 0));
    assert!(mesh.vertices[0].color.r() > 0);
}

#[test]
fn a_glow_with_no_share_or_no_size_draws_nothing() {
    let none = painted(|p| glow_ladder(p, pos2(0.0, 0.0), 20.0, Color32::RED, 0.0, false));
    assert!(meshes(&none).is_empty());
    let flat = painted(|p| glow_ladder(p, pos2(0.0, 0.0), 0.0, Color32::RED, 1.0, false));
    assert!(meshes(&flat).is_empty());
}

#[test]
fn a_smooth_path_runs_through_the_ends_and_cuts_the_corners() {
    let corner = [pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0)];
    let path = smooth_path(&corner, 8);
    assert_eq!(path.first(), Some(&corner[0]));
    assert_eq!(path.last(), Some(&corner[2]));
    assert!(path.len() > 8);
    let nearest = path
        .iter()
        .map(|p| (*p - corner[1]).length())
        .fold(f32::MAX, f32::min);
    assert!(nearest > 1.0, "the corner is rounded: {nearest}");
    assert!(path.iter().all(|p| p.x <= 10.0 + 1e-4 && p.y >= -1e-4));
    assert_eq!(smooth_path(&corner[..2], 8), corner[..2].to_vec());
}
