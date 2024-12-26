use nostrd::NostrD;

fn new_nostrd_instance() -> NostrD {
    std::env::set_var("RUST_LOG", "debug");
    let nostrd = NostrD::new().unwrap();
    println!("NostrD running at {}:{}", nostrd.addr, nostrd.port);
    nostrd
}

#[test]
fn simple_nostrd() {
    let _ = new_nostrd_instance();
}
