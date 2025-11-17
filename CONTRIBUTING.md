# Contributing to `cargo-sandbox`

Thanks for the interest in the project! The project is currently run by me, [@madsmtm](https://github.com/madsmtm), but I am developing this project for the Rust community, with the goal of at some point integrating it into Cargo proper, so I'm very open to contributions.

Feel free to just submit a PR!


## Testing

Simply running `cargo test` should work.

To run a locally built `cargo-sandbox`, make sure to first run `cargo build`. This will build both `cargo-sandbox` and `cargo-sandbox-interceptor`. After that, you should be able to run something like this:

```console
$ $MY_PROJECT_DIR/target/debug/cargo-sandbox build
```

Note that `cargo run` will NOT rebuild the interceptor, so using this for development is discouraged.
