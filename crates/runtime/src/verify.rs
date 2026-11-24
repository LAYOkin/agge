use crate::{Fault, Input, World};
use agge_core::{Project, Result, fail, valid_id};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpan(pub u32, pub u32, pub String);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    pub ticks: u32,
    #[serde(default)]
    pub room: Option<String>,
    #[serde(default)]
    pub input: Vec<InputSpan>,
    #[serde(default)]
    pub expect: Vec<Assertion>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub at: u32,
    pub field: String,
    pub is: Scalar,
    #[serde(default)]
    pub tolerance: f32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Scalar {
    Number(f32),
    Text(String),
}

#[derive(Debug, Serialize)]
pub struct Failure {
    pub at: u32,
    pub field: String,
    pub expected: Scalar,
    pub actual: Option<Scalar>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub ticks: u64,
    pub assertions: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<Failure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<Fault>,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.failures.is_empty() && self.fault.is_none()
    }
}

pub fn replay(project: Project, spec: &Replay) -> Result<(World, Report)> {
    if spec.ticks > 60_000
        || spec.input.len() > 4096
        || spec.expect.len() > 4096
        || spec
            .input
            .iter()
            .any(|InputSpan(a, b, key)| *a == 0 || a > b || *b > spec.ticks || !valid_id(key))
        || spec
            .expect
            .iter()
            .any(|a| a.at > spec.ticks || !a.tolerance.is_finite() || a.tolerance < 0.)
    {
        return Err(fail("invalid replay ticks, input ranges or assertions"));
    }
    let mut world = World::preview(project, spec.room.as_deref())?;
    let mut report = Report {
        ticks: 0,
        assertions: 0,
        failures: vec![],
        fault: world.fault().cloned(),
    };
    if report.fault.is_some() {
        return Ok((world, report));
    }
    let mut assertions = spec.expect.iter().collect::<Vec<_>>();
    assertions.sort_by_key(|a| a.at);
    let mut assertions = assertions.into_iter().peekable();
    let mut changes = spec
        .input
        .iter()
        .flat_map(|InputSpan(first, last, action)| {
            [(*first, action, true), (*last + 1, action, false)]
        })
        .collect::<Vec<_>>();
    changes.sort_unstable_by_key(|(tick, _, _)| *tick);
    let mut changes = changes.into_iter().peekable();
    let mut counts = BTreeMap::new();
    let mut input = Input::new();
    for time in 0..=spec.ticks {
        if time > 0 {
            while let Some((_, action, start)) = changes.next_if(|(tick, _, _)| *tick == time) {
                let count = counts.entry(action).or_insert(0usize);
                if start {
                    if *count == 0 {
                        input.insert(action.clone());
                    }
                    *count += 1;
                } else {
                    *count -= 1;
                    if *count == 0 {
                        input.remove(action);
                    }
                }
            }
            if let Err(error) = world.tick(&input) {
                report.fault = Some(error);
                break;
            }
            report.ticks = u64::from(time);
        }
        while let Some(assertion) = assertions.next_if(|a| a.at == time) {
            report.assertions += 1;
            let actual = read(&world, &assertion.field);
            let equal = match (&actual, &assertion.is) {
                (Some(Scalar::Number(actual)), Scalar::Number(expected)) => {
                    (actual - expected).abs() <= assertion.tolerance
                }
                (Some(actual), expected) => actual == expected,
                (None, _) => false,
            };
            if !equal {
                report.failures.push(Failure {
                    at: time,
                    field: assertion.field.clone(),
                    expected: assertion.is.clone(),
                    actual,
                });
            }
        }
    }
    Ok((world, report))
}

fn read(world: &World, field: &str) -> Option<Scalar> {
    let mut parts = field.split('/');
    let value = match (parts.next()?, parts.next(), parts.next(), parts.next()) {
        ("room", None, None, None) => return Some(Scalar::Text(world.room_id().into())),
        ("particles", None, None, None) => world.particles.len() as f32,
        ("count", Some(object), None, None) => world.count(object) as f32,
        ("grounded", Some(placement), None, None) => {
            u8::from(world.grounded(world.placed(placement)?)) as f32
        }
        ("camera", Some("x"), None, None) => world.camera()[0],
        ("camera", Some("y"), None, None) => world.camera()[1],
        ("global", Some(key), None, None) => world.globals.get(key).copied().unwrap_or(0.),
        ("entity", Some(placement), Some(property), None) => {
            let entity = world.placed(placement)?;
            match property {
                "object" => return Some(Scalar::Text(entity.object.clone())),
                "x" => entity.x,
                "y" => entity.y,
                "vx" => entity.vx,
                "vy" => entity.vy,
                key => *entity.vars.get(key)?,
            }
        }
        _ => return None,
    };
    Some(Scalar::Number(value))
}
