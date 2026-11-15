#[global_allocator]
static ALLOCATOR: lumen_rt::ReportingAllocator = lumen_rt::ReportingAllocator;

fn main() -> eframe::Result {
    lumen_app::run()
}
