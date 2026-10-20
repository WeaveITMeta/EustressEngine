use eustress_avatar_schema::{AvatarDescriptor, AvatarIdentity, BaseBody, RigDefinition};

#[test]
fn identity_is_a_closed_enum_and_round_trips() {
    for identity in AvatarIdentity::ALL {
        let mut d = AvatarDescriptor::default();
        d.select_identity(identity);
        let bytes = serde_json::to_string(&d).unwrap();
        assert!(bytes.contains(&format!("\"identity\":\"{}\"", identity.code())));
        let parsed: AvatarDescriptor = serde_json::from_str(&bytes).unwrap();
        assert_eq!(parsed, d);
        parsed.validate().unwrap();
        assert_eq!(d.resolved_rig().identity, identity);
    }
    assert!(serde_json::from_str::<AvatarIdentity>("\"other\"").is_err());
}

#[test]
fn legacy_female_avatar_preserves_its_rig_and_upgrades_on_selection() {
    for json in [
        r#"{"schema_version":1,"base_body":"feminine"}"#,
        r#"{"base_body":"feminine"}"#,
    ] {
        let mut d: AvatarDescriptor = serde_json::from_str(json).unwrap();
        assert_eq!(d.resolved_identity(), AvatarIdentity::Female);
        assert!(d.resolved_rig().body_asset.ends_with("x_bot.glb"));
        d.select_identity(AvatarIdentity::Robot);
        assert_eq!(d.base_body, BaseBody::Robot);
        assert_eq!(d.schema_version, 2);
        d.validate().unwrap();
    }
}

#[test]
fn custom_rigs_round_trip_and_invalidate_cached_previews() {
    let mut d = AvatarDescriptor::default();
    d.select_identity(AvatarIdentity::Robot);
    let hash = d.content_hash();
    let mut rig = RigDefinition::builtin(AvatarIdentity::Robot);
    rig.id = "industrial_unit".into();
    rig.bone_aliases.push(("Pelvis".into(), "hips".into()));
    d.rig = Some(rig);
    d.validate().unwrap();
    assert_ne!(hash, d.content_hash());
    let encoded = toml::to_string(&d).unwrap();
    assert_eq!(toml::from_str::<AvatarDescriptor>(&encoded).unwrap(), d);
    d.identity = AvatarIdentity::Female;
    assert!(d.validate().is_err());
}

#[test]
fn rig_assets_cannot_escape_the_installed_character_directory() {
    for bad in [
        "bundled://characters/../secret.glb",
        "https://example.com/rig.glb",
        "bundled://characters/C:/rig.glb",
        "bundled://characters/a\\b.glb",
        "bundled://characters/rig.glb#Scene1",
    ] {
        let mut rig = RigDefinition::builtin(AvatarIdentity::Robot);
        rig.body_asset = bad.into();
        assert!(rig.validate().is_err(), "accepted {bad}");
    }
    let mut rig = RigDefinition::builtin(AvatarIdentity::Robot);
    rig.bone_aliases = vec![
        ("Pelvis".into(), "hips".into()),
        ("Root".into(), "hips".into()),
    ];
    assert!(rig.validate().is_err());
}

#[test]
fn space_rig_assets_stay_inside_the_space() {
    let mut rig = RigDefinition::builtin(AvatarIdentity::Male);
    rig.body_asset = "space://StarterPlayer/Characters/BoxheadHero.glb".into();
    rig.validate().unwrap();
    for bad in [
        "space://../escape.glb",
        "space:///absolute.glb",
        "space://C:/rig.glb",
        "space://with space.glb",
        "space://rig.glb#Scene1",
    ] {
        rig.body_asset = bad.into();
        assert!(rig.validate().is_err(), "accepted {bad}");
    }
}

#[test]
fn every_builtin_body_and_motion_is_shipped() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../common/assets");
    for identity in AvatarIdentity::ALL {
        let rig = RigDefinition::builtin(identity);
        rig.validate().unwrap();
        for path in std::iter::once(&rig.body_asset).chain(rig.animations.iter()) {
            let file = assets.join(path.strip_prefix("bundled://").unwrap());
            let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            assert_eq!(&bytes[..4], b"glTF");
            let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
            let gltf: serde_json::Value =
                serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
            if path == &rig.body_asset {
                assert!(!gltf["skins"].as_array().unwrap().is_empty());
            } else {
                assert!(!gltf["animations"][0]["channels"]
                    .as_array()
                    .unwrap()
                    .is_empty());
            }
        }
    }
}
