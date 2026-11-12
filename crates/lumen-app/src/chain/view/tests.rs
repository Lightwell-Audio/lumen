//! The tab's tests.

use super::paint::output_name;
use super::rows::ago;
use super::*;

#[test]
fn an_age_is_said_roughly() {
    use std::time::Duration;
    assert_eq!(ago(Duration::from_secs(2)), "a moment");
    assert_eq!(ago(Duration::from_secs(40)), "40 s");
    assert_eq!(ago(Duration::from_secs(185)), "3 min");
    assert_eq!(ago(Duration::from_secs(7300)), "2 h");
}

#[test]
fn the_output_chip_names_where_the_sound_goes() {
    let mut project = crate::empty_project();
    let color = project.master.color;
    let group = Track::new(Id(1), "Drums".to_owned(), TrackKind::Group, color);
    let mut kick = Track::new(Id(2), "Kick".to_owned(), TrackKind::Audio, color);
    kick.parent = Some(Id(1));
    project.tracks = vec![group.into(), kick.clone().into()];
    assert_eq!(output_name(&project, &kick), "Drums");
    kick.output = Output::Master;
    assert_eq!(output_name(&project, &kick), "Master");
    kick.output = Output::Device(1);
    assert_eq!(output_name(&project, &kick), "Out 3/4");
    kick.output = Output::Track(Id(99));
    assert_eq!(output_name(&project, &kick), "Master");
}
