use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn blocking_file_io_inside_a_real_time_section_is_caught() {
    lumen_rt::rt_section(|| {
        let _ = std::fs::File::open("/dev/null");
    });
}

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn waiting_on_a_held_lock_inside_a_real_time_section_is_caught() {
    let shared = Arc::new(Mutex::new(0));
    let held = shared.lock().unwrap();
    let waiter = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            lumen_rt::rt_section(|| {
                *shared.lock().unwrap() += 1;
            });
        })
    };
    std::thread::sleep(Duration::from_millis(200));
    drop(held);
    waiter.join().unwrap();
}

#[test]
#[ignore = "run by the CI rtsan job, which expects it to abort"]
fn an_allocation_inside_a_real_time_section_is_caught() {
    lumen_rt::rt_section(|| {
        std::hint::black_box(vec![0.0_f32; 256]);
    });
}
