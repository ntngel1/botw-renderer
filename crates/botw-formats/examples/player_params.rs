//! Prints the movement parameters of Link's actor (`GameROMPlayer`, inside
//! `TitleBG.pack`): general parameters, the AI actions' static parameters
//! and the physics' character controller and rigid bodies.
//!
//! `cargo run -p botw-formats --example player_params -- <content dir> [more dirs...]`
//! With `TREE=1` it also lists every AI, action, behaviour and query with
//! its children.

use std::sync::Arc;

use botw_formats::actor::ActorPacks;
use botw_formats::content::ContentRoots;
use botw_formats::params::{AiProgram, CharacterController, GeneralParams, RigidBody};

const GENERAL: &[&str] = &[
    "ClimbEnableAngle",
    "ClimbEnableSpeedMinAngle",
    "ClimbEnableSpeedMaxAngle",
    "VelDiamTired",
    "StickDiamTired",
    "DashToRunStickValueDec",
    "EnergyAutoRecover",
    "EnergyAutoRecoverInAir",
    "EnergyAutoRecoverInvalidTime1",
    "EnergyAutoRecoverInvalidTime2",
    "EnergyTiredValue",
    "MoveMaxDecRateByWater",
    "MoveIgnoreWaterHeight",
    "MoveDecRateMaxHeight",
    "TurnEnableDirSub",
    "ClimbRestartTime",
];

const ACTIONS: &[(&str, &[&str])] = &[
    ("PlayerMove", &["EnergyDash", "EnergyDashTrig"]),
    (
        "PlayerJump",
        &[
            "JumpHeight",
            "JumpHeightAddBySpeed",
            "JumpHeightMaxDecRateByWater",
            "IgnoreWaterHeight",
            "EnergyDashJump",
        ],
    ),
    ("PlayerFall", &["NoClimbTime", "NoClimbTimeTired"]),
    (
        "PlayerNormal",
        &["ParashawlInvalidHeight", "ParashawlInvalidHeightSurfing"],
    ),
    (
        "PlayerActionClimb",
        &[
            "StaminaDownAlways",
            "StaminaDownTriggerJump",
            "StaminaDownWait",
            "StaminaRateMovingUp",
            "StaminaRateMovingSide",
            "StaminaRateMovingDown",
            "StaminaRateSlopeMin",
            "StaminaRateSlopeCenter",
            "StaminaRateSlopeMax",
            "EndGroundAngle",
            "BodyFixedOffset",
            "FallLimitWallAngle",
            "LockingAfterJumpCnt",
            "ChargeJumpScaleMax",
        ],
    ),
    (
        "PlayerParashawlGlide",
        &[
            "EnergyGlide",
            "NoEnergyTime",
            "GlideSpeedMax",
            "Lv2GlideSpeedMax",
        ],
    ),
    (
        "PlayerSwimMove",
        &[
            "MaxSpeedF",
            "MaxSpeedS",
            "MaxSpeedB",
            "MaxSpeedDash",
            "EnergyMove",
            "EnergyDash",
            "DecSpeedRate",
        ],
    ),
    ("PlayerSwimWait", &["EnergyWait", "DecSpeedRate"]),
    ("PlayerSwimDash", &["EnergyDash"]),
    (
        "PlayerSwim",
        &["EnableHeight", "CatchHeightL", "CatchHeightH"],
    ),
];

fn main() {
    let (roots, _) = ContentRoots::resolve(std::env::args().skip(1));
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .and_then(|p| std::fs::read(p).ok())
        .map(Arc::new);
    let packs = ActorPacks::new(roots, title_bg);
    let pack = packs
        .pack("GameROMPlayer")
        .unwrap()
        .expect("no GameROMPlayer pack");
    let sarc = roead::sarc::Sarc::new(&pack[..]).unwrap();
    let file = |prefix: &str| {
        sarc.files()
            .find(|f| f.name().is_some_and(|n| n.starts_with(prefix)))
            .map(|f| f.data().to_vec())
    };

    let general = GeneralParams::parse(&file("Actor/GeneralParamList/").unwrap()).unwrap();
    println!("Player:");
    for name in GENERAL {
        println!("  {name} = {:?}", general.number("Player", name));
    }
    let program = AiProgram::parse(&file("Actor/AIProgram/").unwrap()).unwrap();
    for (class, names) in ACTIONS {
        println!("{class}:");
        for name in *names {
            println!("  {name} = {:?}", program.number(class, name));
        }
    }
    if std::env::var_os("TREE").is_some() {
        for (i, e) in program.entries.iter().enumerate() {
            let kids: Vec<String> = e
                .children()
                .map(|c| {
                    format!(
                        "{c}:{}({})",
                        program.entries[c].name, program.entries[c].class
                    )
                })
                .collect();
            println!(
                "#{i} {:?} {} ({}) -> {}",
                e.kind,
                e.name,
                e.class,
                kids.join(", ")
            );
        }
    }
    let physics = file("Actor/Physics/").unwrap();
    let controller = CharacterController::parse(&physics)
        .unwrap()
        .unwrap_or_default();
    println!(
        "character controller: water effective height {:?}",
        controller.water_effective_height
    );
    for form in &controller.forms {
        println!(
            "  {}: radius {} height {}",
            form.kind, form.radius, form.height
        );
    }
    println!("rigid bodies:");
    for body in RigidBody::parse_all(&physics).unwrap() {
        println!("  {} / {}:", body.set, body.name);
        for shape in &body.shapes {
            println!(
                "    {} {:?} {:?} radius {}",
                shape.kind, shape.from, shape.to, shape.radius
            );
        }
    }
}
