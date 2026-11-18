const AT_REST: f32 = 0.05;

pub(super) fn is_none(decibels: f32) -> bool {
    decibels < AT_REST
}
