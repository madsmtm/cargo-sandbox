use std::{path::Path, process::Stdio};

use crate::{SandboxCommandExt, prelude::*, sandbox_config_file};
use cargo_test_support::{cargo_test, project};

#[cargo_test(public_network_test)]
fn sqlx() {
    let p = project()
        .file(
            sandbox_config_file(),
            r#"
                global = "deny"
            "#,
        )
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2024"

                [features]
                allow = []
                sqlite = ["sqlx/sqlite"]
                postgres = ["sqlx/postgres"]

                [dependencies]
                tokio = { version = "1.20.0", features = ["rt", "macros"]}
                sqlx = { version = "0.8", features = ["runtime-tokio"] }
            "#,
        )
        .file(
            "src/main.rs",
            r#"
                #[tokio::main(flavor = "current_thread")]
                async fn main() -> Result<(), Box<dyn std::error::Error>> {
                    let database_url = std::env::var("DATABASE_URL").unwrap();
                    #[cfg(feature = "sqlite")]
                    let pool = sqlx::SqlitePool::connect(&database_url).await?;
                    #[cfg(feature = "postgres")]
                    let pool = sqlx::PgPool::connect(&database_url).await?;
                    let one = sqlx::query_scalar!("SELECT 1").fetch_one(&pool).await?;
                    #[cfg(feature = "postgres")]
                    let one = one.unwrap();
                    assert_eq!(one, 1);
                    Ok(())
                }
            "#,
        )
        .build();

    // Test simple in-memory database.
    p.cargo("run --features sqlite")
        .env("DATABASE_URL", "sqlite::memory:")
        .with_stderr_does_not_contain("[WARNING]")
        .run();

    // Create empty sqlite database, and allow access to it.
    p.process("sqlite3").arg("foo.db").arg("VACUUM;").run();
    p.sandbox_config(
        r#"
            [[proc-macros.sqlx.paths]]
            path = "foo.db"
            read = "allow"
        "#,
    );
    p.cargo("run --features sqlite")
        .env("DATABASE_URL", "sqlite:foo.db")
        .with_stderr_does_not_contain("[WARNING]")
        .run();

    // Create postgresql database.
    let port = 55432;
    let user = "testuser";
    let db = "testdb";
    let pgdata = Path::new("pgdata");
    p.process("initdb")
        .arg("-D")
        .arg(pgdata)
        .arg("-U")
        .arg(user)
        .run();
    p.change_file(
        pgdata.join("postgresql.conf"),
        &format!("listen_addresses = 'localhost'\nport = {port}\n"),
    );

    // Spawn postgresql instance.
    let mut child = p
        .process("postgres")
        .arg("-D")
        .arg(&pgdata)
        .build_command()
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // 4. wait until ready using pg_isready
    for _ in 0..30 {
        let status = std::process::Command::new("pg_isready")
            .arg("-h")
            .arg("localhost")
            .arg("-p")
            .arg(port.to_string())
            .arg("-U")
            .arg(user)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();

        if status.success() {
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    p.process("createdb")
        .arg("-p")
        .arg(port.to_string())
        .arg("-U")
        .arg(user)
        .arg(db)
        .run();

    p.sandbox_config(
        r#"
            [proc-macros.sqlx.network]
            all = "allow" # TODO: Use `local`
        "#,
    );
    p.cargo("run --features postgres")
        .env(
            "DATABASE_URL",
            format!("postgres://{user}@localhost:{port}/{db}"),
        )
        .with_stderr_does_not_contain("[WARNING]")
        .run();

    p.process("pg_ctl").arg("-D").arg(&pgdata).arg("stop").run();
    child.wait().unwrap();
}
