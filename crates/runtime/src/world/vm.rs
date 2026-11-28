use super::{
    ENTITY_LIMIT, Entity, Fault, FaultOrigin, ScriptFrame, ScriptLocation, World, bounded, flag,
    random,
};
use agge_core::{
    Index,
    script::{
        Event, Expr, Instruction, MAX_PARAMETERS, Math, Op, Program, Statement, ViewProperty,
    },
};
use std::borrow::Cow;

const MAX_EVAL_DEPTH: u8 = 128;
const MAX_TRACE_FRAMES: usize = 16;

fn tile_indices(cell: [f32; 2]) -> Option<[usize; 2]> {
    cell.iter()
        .all(|&v| (0. ..=1023.).contains(&v) && v.fract() == 0.)
        .then(|| cell.map(|v| v as usize))
}

#[derive(Clone, Copy)]
struct Context<'a> {
    index: &'a Index,
    script: &'a str,
    program: &'a Program,
    parameters: &'a [f32],
    impact: f32,
}

struct ScriptError {
    message: Cow<'static, str>,
    line: Option<usize>,
    stack: Vec<ScriptFrame>,
    omitted_frames: usize,
}

impl ScriptError {
    fn new(message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            message: message.into(),
            line: None,
            stack: Vec::new(),
            omitted_frames: 0,
        }
    }
}

impl From<&'static str> for ScriptError {
    fn from(message: &'static str) -> Self {
        Self::new(message)
    }
}

impl World {
    pub(super) fn error(&self, entity: u64, event: Event, message: &str) -> Fault {
        Fault {
            tick: self.tick,
            entity,
            event: event.name(),
            message: message.into(),
            origin: None,
        }
    }

    pub(super) fn run(
        &mut self,
        index: &Index,
        entity: &mut Entity,
        event: Event,
        impact: f32,
    ) -> Result<(), Fault> {
        let Some((script, program)) = self
            .object(&entity.object)
            .script
            .as_ref()
            .and_then(|id| index.programs.get_key_value(id))
        else {
            return Ok(());
        };
        let Some(code) = program.events.get(&event) else {
            return Ok(());
        };
        let context = Context {
            index,
            script,
            program,
            parameters: &[],
            impact,
        };
        self.exec(entity, context, code).map_err(|mut error| {
            error.stack.reverse();
            Fault {
                tick: self.tick,
                event: event.name(),
                entity: entity.id,
                message: error.message.into_owned(),
                origin: Some(Box::new(FaultOrigin {
                    object: entity.object.clone(),
                    placement: entity.placement.clone(),
                    source: ScriptLocation {
                        script: script.clone(),
                        line: error.line.expect("failed instruction has a source line"),
                    },
                    stack: error.stack,
                    omitted_frames: (error.omitted_frames != 0).then_some(error.omitted_frames),
                })),
            }
        })
    }

    fn spend(&mut self) -> Result<(), ScriptError> {
        self.budget = self
            .budget
            .checked_sub(1)
            .ok_or("instruction budget exceeded")?;
        Ok(())
    }

    fn exec(
        &mut self,
        entity: &mut Entity,
        context: Context<'_>,
        code: &[Instruction],
    ) -> Result<(), ScriptError> {
        for entry in code {
            self.exec_one(entity, context, &entry.statement)
                .map_err(|mut error| {
                    error.line.get_or_insert(entry.line);
                    error
                })?;
            if entity.dead {
                break;
            }
        }
        Ok(())
    }

    fn exec_one(
        &mut self,
        entity: &mut Entity,
        context: Context<'_>,
        stmt: &Statement,
    ) -> Result<(), ScriptError> {
        self.spend()?;
        match stmt {
            Statement::Set(key, value) => {
                let value = self.eval(entity, context, value, 0)?;
                match key.as_str() {
                    "x" => entity.x = bounded(value)?,
                    "y" => entity.y = bounded(value)?,
                    "vx" => entity.vx = bounded(value)?,
                    "vy" => entity.vy = bounded(value)?,
                    _ => {
                        entity.vars.insert(key.clone(), value);
                    }
                }
            }
            Statement::Global(key, value) => {
                let value = self.eval(entity, context, value, 0)?;
                self.globals.insert(key.clone(), value);
            }
            Statement::Move(x, y) => {
                let (x, y) = (
                    bounded(self.eval(entity, context, x, 0)?)?,
                    bounded(self.eval(entity, context, y, 0)?)?,
                );
                self.motion(entity, 0, x);
                self.motion(entity, 1, y);
            }
            Statement::Flip(x, y) => {
                let x = self.eval(entity, context, x, 0)? != 0.;
                let y = self.eval(entity, context, y, 0)? != 0.;
                entity.effects.get_or_insert_with(Default::default).flip = Some([x, y]);
            }
            Statement::Tile(map, cell, symbol) => {
                let cell = [
                    self.eval(entity, context, &cell.0, 0)?,
                    self.eval(entity, context, &cell.1, 0)?,
                ];
                let at = tile_indices(cell)
                    .ok_or("tile coordinates must be whole cells from 0 to 1023")?;
                self.set_tile(map, at, *symbol)
                    .map_err(|error| ScriptError::new(error.to_string()))?;
            }
            Statement::Tint(channels) => {
                if let Some(channels) = channels {
                    let mut tint = [0; 4];
                    for (channel, expression) in tint.iter_mut().zip(channels.iter()) {
                        *channel = self
                            .eval(entity, context, expression, 0)?
                            .round()
                            .clamp(0., 255.) as u8;
                    }
                    entity.effects.get_or_insert_with(Default::default).tint = Some(tint);
                } else if let Some(effects) = &mut entity.effects {
                    effects.tint = None;
                }
            }
            Statement::Drop(duration) => {
                let ticks = self.eval(entity, context, duration, 0)?;
                if !(0. ..=600.).contains(&ticks) || ticks.fract() != 0. {
                    return Err("drop needs a whole tick count from 0 to 600".into());
                }
                entity
                    .effects
                    .get_or_insert_with(Default::default)
                    .drop_until = self.tick.saturating_add(ticks as u64);
            }
            Statement::Camera(x, y) => {
                let x = bounded(self.eval(entity, context, x, 0)?)?;
                let y = bounded(self.eval(entity, context, y, 0)?)?;
                self.camera.requested = Some([x, y]);
            }
            Statement::Spawn(object, x, y) => {
                let (x, y) = (
                    bounded(self.eval(entity, context, x, 0)?)?,
                    bounded(self.eval(entity, context, y, 0)?)?,
                );
                if self.queued.len() >= ENTITY_LIMIT {
                    return Err("spawn queue limit exceeded".into());
                }
                self.queued.push_back((object.clone(), None, x, y));
            }
            Statement::Sound(id) => {
                if self.audio.len() < 256 {
                    self.audio.push(id.clone());
                }
            }
            Statement::Animate(id, restart) => {
                let restart = if let Some(expr) = restart {
                    self.eval(entity, context, expr, 0)? != 0.
                } else {
                    false
                };
                let current = entity
                    .active_animation()
                    .map(|(id, _)| id)
                    .or_else(|| self.object(&entity.object).animation.as_deref());
                if restart || current != Some(id) {
                    let effects = entity.effects.get_or_insert_with(Default::default);
                    effects.animation = Some(id.clone());
                    effects.animation_since = self.tick;
                }
            }
            Statement::After(name, value) => {
                let ticks = self.eval(entity, context, value, 0)?;
                if !(0. ..=60_000.).contains(&ticks) || ticks.fract() != 0. {
                    return Err("timer needs a whole tick count from 0 to 60000".into());
                }
                if ticks == 0. {
                    if let Some(effects) = &mut entity.effects {
                        effects.timers.remove(name);
                    }
                } else {
                    let timers = &mut entity.effects.get_or_insert_with(Default::default).timers;
                    if timers.len() == 16 && !timers.contains_key(name) {
                        return Err("instance timer limit exceeded".into());
                    }
                    timers.insert(name.clone(), self.tick.saturating_add(ticks as u64));
                }
            }
            Statement::Emit(id, position) => {
                let at = if let Some(position) = position {
                    [
                        bounded(self.eval(entity, context, &position.0, 0)?)?,
                        bounded(self.eval(entity, context, &position.1, 0)?)?,
                    ]
                } else {
                    [entity.x, entity.y]
                };
                self.emit(id, at);
            }
            Statement::Goto(id) => {
                if self.next_room.is_none() {
                    self.next_room = Some(id.clone());
                }
            }
            Statement::Destroy => entity.dead = true,
            Statement::Repeat(expr, body) => {
                let count = self.eval(entity, context, expr, 0)?;
                if !(0. ..=4096.).contains(&count) || count.fract() != 0. {
                    return Err("repeat count must be an integer from 0 to 4096".into());
                }
                for _ in 0..count as u32 {
                    self.spend()?;
                    self.exec(entity, context, body)?;
                    if entity.dead {
                        break;
                    }
                }
            }
            Statement::If(expr, yes, no) => {
                let value = self.eval(entity, context, expr, 0)?;
                self.exec(entity, context, if value != 0. { yes } else { no })?;
            }
        }
        Ok(())
    }

    fn eval(
        &mut self,
        e: &Entity,
        context: Context<'_>,
        expr: &Expr,
        depth: u8,
    ) -> Result<f32, ScriptError> {
        self.spend()?;
        if depth >= MAX_EVAL_DEPTH {
            return Err("expression and call nesting limit exceeded".into());
        }
        let depth = depth + 1;
        let value = match expr {
            Expr::Number(x) => *x,
            Expr::Parameter(slot) => context.parameters[*slot],
            Expr::Variable(name) => match name.as_str() {
                "x" => e.x,
                "y" => e.y,
                "vx" => e.vx,
                "vy" => e.vy,
                "dt" => 1. / self.project.settings.hz as f32,
                "tick" => self.tick as f32,
                "width" => self.camera.room[0] as f32,
                "height" => self.camera.room[1] as f32,
                _ => *e.vars.get(name).ok_or_else(|| {
                    ScriptError::new(format!("undefined instance variable {name}"))
                })?,
            },
            Expr::Global(name) => self.globals.get(name).copied().unwrap_or(0.),
            Expr::Down(key) => flag(self.input.contains(key)),
            Expr::Pressed(key) => flag(self.input.contains(key) && !self.previous.contains(key)),
            Expr::Released(key) => flag(!self.input.contains(key) && self.previous.contains(key)),
            Expr::Touch(name, offset) => {
                let offset = if let Some(pair) = offset {
                    let (dx, dy) = &**pair;
                    [
                        bounded(self.eval(e, context, dx, depth)?)?,
                        bounded(self.eval(e, context, dy, depth)?)?,
                    ]
                } else {
                    [0., 0.]
                };
                flag(self.touches(e, name, offset))
            }
            Expr::Grounded => flag(self.grounded(e)),
            Expr::Impact => context.impact,
            Expr::AnimationDone => flag(
                self.animation(e)
                    .is_some_and(|(_, clip, elapsed)| clip.done(elapsed)),
            ),
            Expr::Timer(name, expired) => {
                let (remaining, due) = e.timer(name, self.tick);
                if *expired {
                    flag(due)
                } else {
                    remaining as f32
                }
            }
            Expr::Blocked(x, y) => {
                let x = bounded(self.eval(e, context, x, depth)?)?;
                let y = bounded(self.eval(e, context, y, depth)?)?;
                flag(self.blocked(e, x, y))
            }
            Expr::Random => random(&mut self.rng),
            Expr::Tile(map, cell, symbol) => {
                let cell = [
                    self.eval(e, context, &cell.0, depth + 1)?,
                    self.eval(e, context, &cell.1, depth + 1)?,
                ];
                flag(tile_indices(cell).is_some_and(|at| self.meets_tile(map, at, *symbol)))
            }
            Expr::Pointer(property) => self.pointer_property(*property),
            Expr::View(property) => match property {
                ViewProperty::X => self.camera.origin[0],
                ViewProperty::Y => self.camera.origin[1],
                ViewProperty::Width => self.project.settings.size[0] as f32,
                ViewProperty::Height => self.project.settings.size[1] as f32,
            },
            Expr::Math(function, args) => {
                let mut numbers = [0.; 3];
                for (value, arg) in numbers.iter_mut().zip(args) {
                    *value = self.eval(e, context, arg, depth)?;
                }
                let [x, y, z] = numbers;
                match function {
                    Math::Abs => x.abs(),
                    Math::Floor => x.floor(),
                    Math::Ceil => x.ceil(),
                    Math::Sin => x.sin(),
                    Math::Cos => x.cos(),
                    Math::Sqrt => x.sqrt(),
                    Math::Min => x.min(y),
                    Math::Max => x.max(y),
                    Math::Clamp if y <= z => x.clamp(y, z),
                    Math::Clamp => return Err("clamp bounds are reversed".into()),
                }
            }
            Expr::Choose(condition, yes, no) => {
                let test = self.eval(e, context, condition, depth)?;
                return self.eval(e, context, if test != 0. { yes } else { no }, depth);
            }
            Expr::Call(target, args) => {
                let mut parameters = [0.; MAX_PARAMETERS];
                for (value, arg) in parameters.iter_mut().zip(args) {
                    *value = self.eval(e, context, arg, depth)?;
                }
                let (script, program) = match &target.module {
                    Some(module) => (module.as_str(), &context.index.programs[module]),
                    None => (context.script, context.program),
                };
                let (name, function) = program
                    .functions
                    .get_key_value(&target.name)
                    .expect("linked function");
                let callee = Context {
                    index: context.index,
                    script,
                    program,
                    parameters: &parameters,
                    impact: context.impact,
                };
                return self
                    .eval(e, callee, &function.expression, depth)
                    .map_err(|mut error| {
                        if error.stack.len() < MAX_TRACE_FRAMES {
                            error.stack.push(ScriptFrame {
                                script: script.into(),
                                function: name.clone(),
                                line: function.line,
                            });
                        } else {
                            error.omitted_frames += 1;
                        }
                        error
                    });
            }
            Expr::Neg(x) => -self.eval(e, context, x, depth)?,
            Expr::Not(x) => flag(self.eval(e, context, x, depth)? == 0.),
            Expr::Binary(op, a, b) => {
                let a = self.eval(e, context, a, depth)?;
                match op {
                    Op::Or if a != 0. => return Ok(1.),
                    Op::And if a == 0. => return Ok(0.),
                    _ => (),
                }
                let b = self.eval(e, context, b, depth)?;
                match op {
                    Op::Or | Op::And => flag(b != 0.),
                    Op::Eq => flag(a == b),
                    Op::Ne => flag(a != b),
                    Op::Lt => flag(a < b),
                    Op::Le => flag(a <= b),
                    Op::Gt => flag(a > b),
                    Op::Ge => flag(a >= b),
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    Op::Rem => a % b,
                }
            }
        };
        if value.is_finite() {
            Ok(value)
        } else {
            Err("non-finite script result".into())
        }
    }
}
