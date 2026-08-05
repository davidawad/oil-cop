pub mod bd;
pub mod gc;
pub mod proc;

/// Resolve a `--rig` argument the same way `gc` itself does: a filesystem
/// path if one exists, otherwise a registered rig name looked up via
/// `gc rig list`.
pub fn resolve_rig(city: Option<&str>, rig: &str) -> anyhow::Result<gc::RigRaw> {
    if std::path::Path::new(rig).is_dir() {
        return Ok(gc::RigRaw {
            name: rig.to_string(),
            path: rig.to_string(),
            prefix: None,
            suspended: false,
            running: None,
        });
    }
    let (list, _) = gc::rig_list(city)?;
    list.rigs
        .into_iter()
        .find(|r| r.name == rig)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no rig named '{rig}' registered with this city, and it isn't a directory either"
            )
        })
}
