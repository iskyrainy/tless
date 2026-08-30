use tless::cmd;

fn main() {
    tless::init_logging();
    cmd::parse_cmd();
}
