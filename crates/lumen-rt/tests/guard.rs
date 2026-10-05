use lumen_rt::{count_rt_violations, guard_is_active, in_rt_section, rt_section, total_violations};

lumen_rt::install_guarded_allocator!();

#[test]
fn guard_is_installed() {
    assert!(guard_is_active());
}

#[test]
fn allocation_outside_section_is_not_a_violation() {
    let ((), seen) = count_rt_violations(|| {});
    assert_eq!(seen, 0);
    let v: Vec<u32> = Vec::with_capacity(64);
    drop(v);
    assert!(!in_rt_section());
}

#[test]
fn allocation_inside_section_is_counted() {
    let (v, seen) = count_rt_violations(|| Vec::<u32>::with_capacity(64));
    assert!(seen >= 1, "allocation went unnoticed");
    drop(v);
}

#[test]
fn deallocation_inside_section_is_counted() {
    let boxed = Box::new([0_u8; 32]);
    let ((), seen) = count_rt_violations(move || drop(boxed));
    assert_eq!(seen, 1);
}

#[test]
fn work_within_preallocated_capacity_is_clean() {
    let mut buffer: Vec<f32> = Vec::with_capacity(1024);
    let ((), seen) = count_rt_violations(|| {
        for i in 0..1024 {
            buffer.push(i as f32);
        }
        buffer.clear();
    });
    assert_eq!(seen, 0);
}

#[test]
fn growth_beyond_capacity_is_counted() {
    let mut buffer: Vec<f32> = Vec::with_capacity(4);
    let ((), seen) = count_rt_violations(|| buffer.extend_from_slice(&[0.0; 16]));
    assert!(seen >= 1);
}

#[test]
fn sections_nest() {
    rt_section(|| {
        assert!(in_rt_section());
        rt_section(|| assert!(in_rt_section()));
        assert!(in_rt_section());
    });
    assert!(!in_rt_section());
}

#[test]
fn violations_on_other_threads_reach_the_global_count() {
    let seen_on_thread = std::thread::spawn(|| {
        rt_section(|| std::hint::black_box(Box::new(1_u64)));
        lumen_rt::thread_violations()
    })
    .join()
    .unwrap();
    assert!(seen_on_thread >= 1);
    // Other tests run concurrently, so only a lower bound is reliable here.
    assert!(total_violations() >= seen_on_thread);
}
