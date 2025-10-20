fn main() {
    dbg!(std::env::vars().collect::<Vec<_>>());

    let env = env!("TMPDIR");
    assert_ne!(env, "123");
}
