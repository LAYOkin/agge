use std::{fs, process::Command};

#[test]
fn capture_refuses_to_overwrite_the_project_or_an_alias() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("game.agge");
    let hardlink = directory.path().join("hardlink.png");
    let contents = b"agge/1 {}\nroom main {}\n";
    fs::write(&source, contents).unwrap();
    fs::hard_link(&source, &hardlink).unwrap();
    #[cfg(unix)]
    let symlink = {
        let path = directory.path().join("symlink.png");
        std::os::unix::fs::symlink(&source, &path).unwrap();
        path
    };
    let targets = [
        &source,
        &hardlink,
        #[cfg(unix)]
        &symlink,
    ];
    for target in targets {
        let result = Command::new(env!("CARGO_BIN_EXE_agge"))
            .arg("capture")
            .arg(&source)
            .arg(r#"{"ticks":0,"crop":[0,0,1,1]}"#)
            .arg(target)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("capture output would overwrite the project")
        );
        assert_eq!(fs::read(&source).unwrap(), contents);
    }
}
