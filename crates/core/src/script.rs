use crate::{Kind, Result, fail, valid_id};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Create,
    Step,
    HitX,
    HitY,
}

impl Event {
    pub fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Step => "step",
            Self::HitX => "hit_x",
            Self::HitY => "hit_y",
        }
    }
}

#[derive(Clone, Debug)]
pub struct EventLocation {
    pub line: usize,
    pub count: usize,
    start: usize,
    body: usize,
    end: usize,
}

impl EventLocation {
    pub fn bytes(&self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
}

#[derive(Clone, Copy)]
enum Scope {
    Var,
    Global,
    Action,
    Timer,
    Function,
}
impl Scope {
    fn name(self) -> &'static str {
        match self {
            Self::Var => "var",
            Self::Global => "global",
            Self::Action => "action",
            Self::Timer => "timer",
            Self::Function => "fn",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub kind: Kind,
    pub id: String,
    pub line: usize,
    pub span: Range<usize>,
    pub quoted: bool,
}

#[derive(Clone, Debug)]
pub struct GlobalUse {
    pub name: String,
    pub line: usize,
    pub span: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct TileKey {
    pub map: String,
    pub symbol: char,
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub events: BTreeMap<Event, Vec<Instruction>>,
    pub functions: BTreeMap<String, Function>,
    pub calls: Vec<Call>,
    pub references: Vec<Reference>,
    pub globals: Vec<GlobalUse>,
    pub tile_keys: Vec<TileKey>,
    pub save_lines: Vec<usize>,
    pub touch_targets: BTreeSet<String>,
    pub locations: BTreeMap<Event, EventLocation>,
    pub symbols: BTreeMap<String, Vec<(usize, Access)>>,
}

#[derive(Clone, Debug)]
pub struct Instruction {
    pub line: usize,
    pub statement: Statement,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub parameters: Vec<String>,
    pub line: usize,
    pub expression: Expr,
}

pub const MAX_PARAMETERS: usize = 8;

#[derive(Clone, Debug)]
pub struct FunctionName {
    pub module: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Call {
    pub target: FunctionName,
    pub line: usize,
    pub arity: usize,
}

#[derive(Clone, Debug)]
pub enum Statement {
    Set(String, Expr),
    Global(String, Expr),
    Move(Expr, Expr),
    Flip(Expr, Expr),
    Tint(Option<Box<[Expr; 4]>>),
    Tile(String, Box<(Expr, Expr)>, char),
    Drop(Expr),
    Camera(Expr, Expr),
    Spawn(String, Expr, Expr),
    Emit(String, Option<Box<(Expr, Expr)>>),
    Sound(String),
    SaveProgress,
    Animate(String, Option<Expr>),
    After(String, Expr),
    Goto(String),
    Destroy,
    If(Expr, Vec<Instruction>, Vec<Instruction>),
    Repeat(Expr, Vec<Instruction>),
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Copy, Debug)]
pub enum Math {
    Abs,
    Floor,
    Ceil,
    Sin,
    Cos,
    Sqrt,
    Min,
    Max,
    Clamp,
}

impl Math {
    fn function(name: &str) -> Option<(Self, usize)> {
        Some(match name {
            "abs" => (Self::Abs, 1),
            "floor" => (Self::Floor, 1),
            "ceil" => (Self::Ceil, 1),
            "sin" => (Self::Sin, 1),
            "cos" => (Self::Cos, 1),
            "sqrt" => (Self::Sqrt, 1),
            "min" => (Self::Min, 2),
            "max" => (Self::Max, 2),
            "clamp" => (Self::Clamp, 3),
            _ => return None,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub enum ViewProperty {
    X,
    Y,
    Width,
    Height,
}

#[derive(Clone, Copy, Debug)]
pub enum PointerProperty {
    Active,
    X,
    Y,
    WorldX,
    WorldY,
}

impl PointerProperty {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "active" => Self::Active,
            "x" => Self::X,
            "y" => Self::Y,
            "world_x" => Self::WorldX,
            "world_y" => Self::WorldY,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    Number(f32),
    Variable(String),
    Parameter(usize),
    Global(String),
    Down(String),
    Pressed(String),
    Released(String),
    Touch(String, Option<Box<(Expr, Expr)>>),
    Grounded,
    Impact,
    AnimationDone,
    Timer(String, bool),
    Blocked(Box<Expr>, Box<Expr>),
    Random,
    View(ViewProperty),
    Tile(String, Box<(Expr, Expr)>, Option<char>),
    Pointer(PointerProperty),
    Math(Math, Vec<Expr>),
    Choose(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(FunctionName, Vec<Expr>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Word(String),
    Number(String),
    Quoted(String),
    Punct(char),
    Pair(char, char),
}

struct Parser<'a> {
    input: &'a str,
    offset: usize,
    line: usize,
    tokens: Vec<(Token, Range<usize>)>,
    at: usize,
    program: &'a mut Program,
    parameters: Option<Vec<String>>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, offset: usize, line: usize, program: &'a mut Program) -> Result<Self> {
        let mut parser = Self {
            input,
            offset,
            line,
            tokens: vec![],
            at: 0,
            program,
            parameters: None,
        };
        let bytes = input.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            let start = at;
            let c = bytes[at] as char;
            at += 1;
            let token = match c {
                ' ' | '\t' => continue,
                '#' => break,
                'a'..='z' | 'A'..='Z' | '_' => {
                    while at < bytes.len()
                        && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_')
                    {
                        at += 1;
                    }
                    Token::Word(input[start..at].into())
                }
                '0'..='9' | '.' if c != '.' || bytes.get(at).is_some_and(u8::is_ascii_digit) => {
                    while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'.') {
                        at += 1;
                    }
                    Token::Number(input[start..at].into())
                }
                '"' => {
                    while at < bytes.len() {
                        let next = bytes[at];
                        at += 1;
                        if next == b'"' {
                            break;
                        }
                        if next == b'\\' && at < bytes.len() {
                            at += 1;
                        }
                    }
                    Token::Quoted(
                        serde_json::from_str(&input[start..at])
                            .map_err(|_| parser.error("invalid quoted string"))?,
                    )
                }
                '!' | '=' | '<' | '>' | '&' | '|'
                    if at < bytes.len()
                        && (bytes[at] == b'=' && !matches!(c, '&' | '|')
                            || matches!(c, '&' | '|') && bytes[at] == bytes[start]) =>
                {
                    at += 1;
                    Token::Pair(c, bytes[at - 1] as char)
                }
                '(' | ')' | ',' | ':' | '+' | '-' | '*' | '/' | '%' | '!' | '=' | '<' | '>'
                | '.' => Token::Punct(c),
                _ => return Err(parser.error("unexpected character")),
            };
            parser.tokens.push((token, start..at));
            if parser.tokens.len() > 128 {
                return Err(parser.error("line exceeds 128 tokens"));
            }
        }
        Ok(parser)
    }
    fn error(&self, message: &str) -> Box<dyn std::error::Error + Send + Sync> {
        fail(format!("{}: {message}", self.line))
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(token, _)| token)
    }
    fn take(&mut self) -> Option<(Token, Range<usize>)> {
        let next = self.tokens.get(self.at).cloned()?;
        self.at += 1;
        Some(next)
    }
    fn punct(&mut self, c: char) -> Result<()> {
        if self.take().is_some_and(|(t, _)| t == Token::Punct(c)) {
            Ok(())
        } else {
            Err(self.error(&format!("expected '{c}'")))
        }
    }
    fn name(&mut self) -> Result<String> {
        match self.take() {
            Some((Token::Word(word), _)) if valid_id(&word) => Ok(word),
            _ => Err(self.error("expected an identifier")),
        }
    }

    fn global_name(&mut self) -> Result<String> {
        let index = self.at;
        let name = self.name()?;
        let span = &self.tokens[index].1;
        self.program.globals.push(GlobalUse {
            name: name.clone(),
            line: self.line,
            span: self.offset + span.start..self.offset + span.end,
        });
        Ok(name)
    }
    fn arguments(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        if self.peek() != Some(&Token::Punct(')')) {
            loop {
                if args.len() == MAX_PARAMETERS {
                    return Err(self.error("too many arguments"));
                }
                args.push(self.expr(0)?);
                if self.peek() != Some(&Token::Punct(',')) {
                    break;
                }
                self.punct(',')?;
            }
        }
        Ok(args)
    }

    fn function(&mut self) -> Result<Line> {
        let name = self.name()?;
        if matches!(
            name.as_str(),
            "rand"
                | "global"
                | "touch"
                | "grounded"
                | "impact"
                | "anim_done"
                | "remaining"
                | "expired"
                | "blocked"
                | "down"
                | "pressed"
                | "choose"
                | "true"
                | "false"
        ) || Math::function(&name).is_some()
        {
            return Err(self.error("function name is reserved"));
        }
        self.punct('(')?;
        let mut parameters = Vec::new();
        if self.peek() != Some(&Token::Punct(')')) {
            loop {
                if parameters.len() == MAX_PARAMETERS {
                    return Err(self.error("too many parameters"));
                }
                let parameter = self.name()?;
                if engine_var(&parameter) || matches!(parameter.as_str(), "true" | "false") {
                    return Err(self.error("parameter name is reserved"));
                }
                if parameters.contains(&parameter) {
                    return Err(self.error(&format!("duplicate parameter {parameter}")));
                }
                parameters.push(parameter);
                if self.peek() != Some(&Token::Punct(',')) {
                    break;
                }
                self.punct(',')?;
            }
        }
        self.punct(')')?;
        self.punct('=')?;
        self.parameters = Some(parameters);
        let expression = self.expr(0)?;
        let parameters = self
            .parameters
            .take()
            .expect("function parameters in scope");
        self.symbol(Scope::Function, &name, Access::Write);
        Ok(Line::Function(
            name,
            Function {
                parameters,
                line: self.line,
                expression,
            },
        ))
    }

    fn user_call(&mut self, target: FunctionName) -> Result<Expr> {
        let args = self.arguments()?;
        if target.module.is_none() {
            self.symbol(Scope::Function, &target.name, Access::Read);
        }
        self.program.calls.push(Call {
            target: target.clone(),
            line: self.line,
            arity: args.len(),
        });
        Ok(Expr::Call(target, args))
    }
    fn symbol(&mut self, scope: Scope, name: &str, access: Access) {
        let uses = self
            .program
            .symbols
            .entry(format!("{}/{name}", scope.name()))
            .or_default();
        uses.push((self.line, access));
    }
    fn reference(&mut self, kind: Kind, quoted: bool) -> Result<String> {
        let (token, span) = self
            .take()
            .ok_or_else(|| self.error("expected resource ID"))?;
        let id = match (quoted, token) {
            (false, Token::Word(id)) | (true, Token::Quoted(id)) if valid_id(&id) => id,
            _ => return Err(self.error("expected resource ID")),
        };
        self.program.references.push(Reference {
            kind,
            id: id.clone(),
            line: self.line,
            span: self.offset + span.start..self.offset + span.end,
            quoted,
        });
        Ok(id)
    }
    fn tile_key(&mut self, map: &str) -> Result<char> {
        let Some((Token::Quoted(text), _)) = self.take() else {
            return Err(self.error("expected a quoted tile glyph"));
        };
        let [byte] = text.as_bytes() else {
            return Err(self.error("tile glyph must be one printable ASCII character"));
        };
        if !byte.is_ascii_graphic() {
            return Err(self.error("tile glyph must be one printable ASCII character"));
        }
        let symbol = char::from(*byte);
        self.program.tile_keys.push(TileKey {
            map: map.into(),
            symbol,
            line: self.line,
        });
        Ok(symbol)
    }
    fn end(&self) -> Result<()> {
        if self.peek().is_none() {
            Ok(())
        } else {
            Err(self.error("unexpected content at end of line"))
        }
    }
    fn xy(&mut self) -> Result<(Expr, Expr)> {
        let x = self.expr(0)?;
        self.punct(',')?;
        Ok((x, self.expr(0)?))
    }
    fn expr(&mut self, minimum: u8) -> Result<Expr> {
        let mut left = match self.take() {
            Some((Token::Word(module), span))
                if valid_id(&module) && self.peek() == Some(&Token::Punct('.')) =>
            {
                self.punct('.')?;
                let name = self.name()?;
                self.punct('(')?;
                self.program.references.push(Reference {
                    kind: Kind::Script,
                    id: module.clone(),
                    line: self.line,
                    span: self.offset + span.start..self.offset + span.end,
                    quoted: false,
                });
                let call = self.user_call(FunctionName {
                    module: Some(module),
                    name,
                })?;
                self.punct(')')?;
                call
            }
            Some((Token::Number(value), _)) => Expr::Number(
                value
                    .parse::<f32>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| self.error("invalid number"))?,
            ),
            Some((Token::Punct('-'), _)) => Expr::Neg(Box::new(self.expr(8)?)),
            Some((Token::Punct('!'), _)) => Expr::Not(Box::new(self.expr(8)?)),
            Some((Token::Punct('('), _)) => {
                let e = self.expr(0)?;
                self.punct(')')?;
                e
            }
            Some((Token::Word(word), _)) if word == "true" => Expr::Number(1.),
            Some((Token::Word(word), _)) if word == "false" => Expr::Number(0.),
            Some((Token::Word(word), _)) if valid_id(&word) => {
                if self.peek() == Some(&Token::Punct('(')) {
                    self.punct('(')?;
                    let call = match word.as_str() {
                        "rand" => Expr::Random,
                        "global" => {
                            let name = self.global_name()?;
                            self.symbol(Scope::Global, &name, Access::Read);
                            Expr::Global(name)
                        }
                        "grounded" => Expr::Grounded,
                        "impact" => Expr::Impact,
                        "anim_done" => Expr::AnimationDone,
                        "remaining" | "expired" => {
                            let name = self.name()?;
                            self.symbol(Scope::Timer, &name, Access::Read);
                            Expr::Timer(name, word == "expired")
                        }
                        "blocked" => {
                            let (x, y) = self.xy()?;
                            Expr::Blocked(Box::new(x), Box::new(y))
                        }
                        "touch" => {
                            let id = self.reference(Kind::Object, true)?;
                            self.program.touch_targets.insert(id.clone());
                            let offset = if self.peek() == Some(&Token::Punct(',')) {
                                self.punct(',')?;
                                Some(Box::new(self.xy()?))
                            } else {
                                None
                            };
                            Expr::Touch(id, offset)
                        }
                        "tile_is" | "tile_solid"
                            if matches!(self.peek(), Some(Token::Quoted(_))) =>
                        {
                            let id = self.reference(Kind::Tilemap, true)?;
                            self.punct(',')?;
                            let xy = Box::new(self.xy()?);
                            let key = if word == "tile_is" {
                                self.punct(',')?;
                                Some(self.tile_key(&id)?)
                            } else {
                                None
                            };
                            Expr::Tile(id, xy, key)
                        }
                        "pointer" if matches!(self.peek(), Some(Token::Quoted(_))) => {
                            let Some((Token::Quoted(key), _)) = self.take() else {
                                unreachable!()
                            };
                            Expr::Pointer(PointerProperty::parse(&key).ok_or_else(|| {
                                self.error(
                                    "pointer property must be active, x, y, world_x or world_y",
                                )
                            })?)
                        }
                        "view" if matches!(self.peek(), Some(Token::Quoted(_))) => {
                            let Some((Token::Quoted(key), _)) = self.take() else {
                                unreachable!()
                            };
                            Expr::View(match key.as_str() {
                                "x" => ViewProperty::X,
                                "y" => ViewProperty::Y,
                                "width" => ViewProperty::Width,
                                "height" => ViewProperty::Height,
                                _ => {
                                    return Err(
                                        self.error("view property must be x, y, width or height")
                                    );
                                }
                            })
                        }
                        "down" | "pressed" | "released"
                            if word != "released"
                                || matches!(self.peek(), Some(Token::Quoted(_))) =>
                        {
                            let Some((Token::Quoted(key), _)) = self.take() else {
                                return Err(self.error("expected quoted action"));
                            };
                            if !valid_id(&key) {
                                return Err(self.error("invalid action name"));
                            }
                            self.symbol(Scope::Action, &key, Access::Read);
                            match word.as_str() {
                                "down" => Expr::Down(key),
                                "pressed" => Expr::Pressed(key),
                                _ => Expr::Released(key),
                            }
                        }
                        "choose" => {
                            let condition = self.expr(0)?;
                            self.punct(',')?;
                            let yes = self.expr(0)?;
                            self.punct(',')?;
                            Expr::Choose(
                                Box::new(condition),
                                Box::new(yes),
                                Box::new(self.expr(0)?),
                            )
                        }
                        _ => match Math::function(&word) {
                            Some((math, arity)) => {
                                let mut args = vec![self.expr(0)?];
                                for _ in 1..arity {
                                    self.punct(',')?;
                                    args.push(self.expr(0)?);
                                }
                                Expr::Math(math, args)
                            }
                            None => self.user_call(FunctionName {
                                module: None,
                                name: word,
                            })?,
                        },
                    };
                    self.punct(')')?;
                    call
                } else if engine_var(&word) {
                    Expr::Variable(word)
                } else if let Some(parameters) = &self.parameters {
                    let slot = parameters
                        .iter()
                        .position(|name| name == &word)
                        .ok_or_else(|| self.error(&format!("unknown parameter {word}")))?;
                    Expr::Parameter(slot)
                } else {
                    self.symbol(Scope::Var, &word, Access::Read);
                    Expr::Variable(word)
                }
            }
            _ => return Err(self.error("expected an expression")),
        };
        while let Some((op, priority)) = self.peek().and_then(operator) {
            if priority < minimum {
                break;
            }
            self.take();
            left = Expr::Binary(op, Box::new(left), Box::new(self.expr(priority + 1)?));
        }
        Ok(left)
    }
}

fn engine_var(word: &str) -> bool {
    matches!(
        word,
        "x" | "y" | "vx" | "vy" | "dt" | "tick" | "width" | "height"
    )
}

fn operator(t: &Token) -> Option<(Op, u8)> {
    use Op::*;
    Some(match t {
        Token::Pair('|', '|') => (Or, 1),
        Token::Pair('&', '&') => (And, 2),
        Token::Pair('=', '=') => (Eq, 3),
        Token::Pair('!', '=') => (Ne, 3),
        Token::Punct('<') => (Lt, 4),
        Token::Punct('>') => (Gt, 4),
        Token::Pair('<', '=') => (Le, 4),
        Token::Pair('>', '=') => (Ge, 4),
        Token::Punct('+') => (Add, 5),
        Token::Punct('-') => (Sub, 5),
        Token::Punct('*') => (Mul, 6),
        Token::Punct('/') => (Div, 6),
        Token::Punct('%') => (Rem, 6),
        _ => return None,
    })
}

enum Line {
    Event(Event),
    Function(String, Function),
    Statement(Statement),
    If(Expr),
    Repeat(Expr),
    Else,
    End,
}

fn parse_line(mut p: Parser<'_>) -> Result<Option<Line>> {
    if p.tokens.is_empty() {
        return Ok(None);
    }
    let head = p.name()?;
    use Statement::*;
    let line = match head.as_str() {
        "create" | "step" | "hit_x" | "hit_y" => {
            p.punct(':')?;
            Line::Event(match head.as_str() {
                "create" => Event::Create,
                "step" => Event::Step,
                "hit_x" => Event::HitX,
                _ => Event::HitY,
            })
        }
        "fn" => p.function()?,
        "if" => Line::If(p.expr(0)?),
        "repeat" => Line::Repeat(p.expr(0)?),
        "else" => Line::Else,
        "end" => Line::End,
        other => Line::Statement(match other {
            "set" | "global" => {
                let name = if head == "global" {
                    p.global_name()?
                } else {
                    p.name()?
                };
                if head == "set"
                    && matches!(
                        name.as_str(),
                        "dt" | "tick" | "width" | "height" | "true" | "false"
                    )
                {
                    return Err(p.error("read-only variable"));
                }
                p.punct('=')?;
                let expr = p.expr(0)?;
                if head == "set" {
                    if !matches!(name.as_str(), "x" | "y" | "vx" | "vy") {
                        p.symbol(Scope::Var, &name, Access::Write);
                    }
                    Set(name, expr)
                } else {
                    p.symbol(Scope::Global, &name, Access::Write);
                    Global(name, expr)
                }
            }
            "move" => {
                let (x, y) = p.xy()?;
                Move(x, y)
            }
            "flip" => {
                let (x, y) = p.xy()?;
                Flip(x, y)
            }
            "tile" => {
                let id = p.reference(Kind::Tilemap, false)?;
                let cell = Box::new(p.xy()?);
                p.punct(',')?;
                let symbol = p.tile_key(&id)?;
                Tile(id, cell, symbol)
            }
            "tint" => {
                if p.peek().is_some() {
                    let (r, g) = p.xy()?;
                    p.punct(',')?;
                    let (b, a) = p.xy()?;
                    Tint(Some(Box::new([r, g, b, a])))
                } else {
                    Tint(None)
                }
            }
            "drop" => Drop(p.expr(0)?),
            "camera" => {
                let (x, y) = p.xy()?;
                Camera(x, y)
            }
            "spawn" => {
                let id = p.reference(Kind::Object, false)?;
                let (x, y) = p.xy()?;
                Spawn(id, x, y)
            }
            "emit" => {
                let id = p.reference(Kind::Particle, false)?;
                let position = if p.peek() == Some(&Token::Punct(',')) {
                    p.punct(',')?;
                    Some(Box::new(p.xy()?))
                } else {
                    None
                };
                Emit(id, position)
            }
            "sound" => Sound(p.reference(Kind::Audio, false)?),
            "save" => {
                p.program.save_lines.push(p.line);
                SaveProgress
            }
            "animate" => {
                let id = p.reference(Kind::Animation, false)?;
                let restart = if p.peek() == Some(&Token::Punct(',')) {
                    p.punct(',')?;
                    Some(p.expr(0)?)
                } else {
                    None
                };
                Animate(id, restart)
            }
            "after" => {
                let name = p.name()?;
                p.symbol(Scope::Timer, &name, Access::Write);
                p.punct(',')?;
                After(name, p.expr(0)?)
            }
            "goto" => Goto(p.reference(Kind::Room, false)?),
            "destroy" => Destroy,
            _ => return Err(p.error(&format!("unknown statement in {}", p.input.trim()))),
        }),
    };
    p.end()?;
    Ok(Some(line))
}

pub fn compile(source: &str) -> Result<Program> {
    let program = parse(source)?;
    for call in &program.calls {
        if call.target.module.is_none()
            && let Some(message) = program.call_error(call)
        {
            return Err(fail(format!("{}: {message}", call.line)));
        }
    }
    Ok(program)
}

impl Program {
    pub(crate) fn call_error(&self, call: &Call) -> Option<String> {
        let name = &call.target.name;
        let label = || {
            call.target
                .module
                .as_ref()
                .map_or_else(|| name.clone(), |module| format!("{module}.{name}"))
        };
        match self.functions.get(name) {
            None => Some(format!("unknown function {}", label())),
            Some(function) if function.parameters.len() != call.arity => Some(format!(
                "{} takes {} arguments, got {}",
                label(),
                function.parameters.len(),
                call.arity
            )),
            _ => None,
        }
    }
}

pub(crate) fn parse(source: &str) -> Result<Program> {
    const MAX_LINES: usize = 4096;
    if source.len() > 128 * 1024 {
        return Err(fail("1: script exceeds 128 KiB"));
    }
    let mut program = Program::default();
    let mut event = None;
    let mut block = Vec::new();
    enum BlockKind {
        If(Expr, Option<Vec<Instruction>>),
        Repeat(Expr),
    }
    let mut stack = Vec::<(usize, BlockKind, Vec<Instruction>)>::new();
    let mut offset = 0;
    for (index, text) in source.split_inclusive('\n').enumerate() {
        let line = index + 1;
        if line > MAX_LINES {
            return Err(fail(format!("{line}: too many script lines")));
        }
        let result = parse_line(Parser::new(
            text.trim_end_matches(['\r', '\n']),
            offset,
            line,
            &mut program,
        )?)?;
        let start = offset;
        offset += text.len();
        let Some(result) = result else { continue };
        let header = matches!(result, Line::Event(_) | Line::Function(..));
        if event.is_none() && !header {
            return Err(fail(format!(
                "{line}: expected an event or function declaration"
            )));
        }
        if !header && let Some(location) = event.and_then(|event| program.locations.get_mut(&event))
        {
            location.count = line - location.line + 1;
            location.end = offset;
        }
        match result {
            Line::Event(next) if stack.is_empty() => {
                if let Some(previous) = event.replace(next) {
                    program.events.insert(previous, std::mem::take(&mut block));
                }
                if program
                    .locations
                    .insert(
                        next,
                        EventLocation {
                            line,
                            count: 1,
                            start,
                            body: offset,
                            end: offset,
                        },
                    )
                    .is_some()
                {
                    return Err(fail(format!("{line}: duplicate event")));
                }
            }
            Line::Function(name, function) if stack.is_empty() => {
                if let Some(previous) = event.take() {
                    program.events.insert(previous, std::mem::take(&mut block));
                }
                if program.functions.insert(name.clone(), function).is_some() {
                    return Err(fail(format!("{line}: duplicate function {name}")));
                }
            }
            Line::Event(_) | Line::Function(..) => {
                return Err(fail(format!("{line}: expected end before declaration")));
            }
            Line::Statement(statement) => block.push(Instruction { line, statement }),
            Line::If(expr) if stack.len() < 16 => {
                stack.push((line, BlockKind::If(expr, None), std::mem::take(&mut block)))
            }
            Line::Repeat(expr) if stack.len() < 16 => {
                stack.push((line, BlockKind::Repeat(expr), std::mem::take(&mut block)))
            }
            Line::If(_) | Line::Repeat(_) => {
                return Err(fail(format!("{line}: block nesting exceeds 16")));
            }
            Line::Else => {
                let Some((_, BlockKind::If(_, yes), _)) = stack.last_mut() else {
                    return Err(fail(format!("{line}: else without if")));
                };
                if yes.is_some() {
                    return Err(fail(format!("{line}: duplicate else")));
                }
                *yes = Some(std::mem::take(&mut block));
            }
            Line::End => {
                let (start_line, kind, mut parent) = stack
                    .pop()
                    .ok_or_else(|| fail(format!("{line}: end without block")))?;
                let stmt = match kind {
                    BlockKind::If(expr, Some(yes)) => Statement::If(expr, yes, block),
                    BlockKind::If(expr, None) => Statement::If(expr, block, vec![]),
                    BlockKind::Repeat(expr) => Statement::Repeat(expr, block),
                };
                parent.push(Instruction {
                    line: start_line,
                    statement: stmt,
                });
                block = parent;
            }
        }
    }
    if let Some((line, ..)) = stack.last() {
        return Err(fail(format!("{line}: unclosed block")));
    }
    if let Some(event) = event {
        program.events.insert(event, block);
    }
    for uses in program.symbols.values_mut() {
        uses.sort_unstable();
        uses.dedup();
    }
    Ok(program)
}

pub fn replace_event(source: &str, event: Event, body: Option<&str>) -> Result<String> {
    let program = parse(source)?;
    let event_location = program.locations.get(&event);
    let Some(body) = body else {
        let location =
            event_location.ok_or_else(|| fail(format!("absent {} event", event.name())))?;
        let mut edited = source.to_owned();
        edited.replace_range(location.start..location.end, "");
        return Ok(edited.trim_end_matches('\n').to_owned());
    };
    let body = body.replace("\r\n", "\n");
    let body = body.trim_matches('\n');
    let header = event.name();
    let addition = format!("{header}:\n{body}\n");
    let body_program = parse(&addition)?;
    if body_program.locations.len() != 1 || !body_program.functions.is_empty() {
        return Err(fail("event body contains a declaration"));
    }
    let mut edited = source.to_owned();
    if let Some(location) = event_location {
        let mut replacement = String::new();
        if !body.is_empty() {
            if !source[..location.body].ends_with('\n') {
                replacement.push('\n');
            }
            replacement.push_str(body);
            replacement.push('\n');
        }
        edited.replace_range(location.body..location.end, &replacement);
    } else {
        if !edited.is_empty() && !edited.ends_with('\n') {
            edited.push('\n');
        }
        edited.push_str(&addition);
    }
    Ok(edited.trim_end_matches('\n').to_owned())
}

pub fn splice_lines(source: &str, [first, count]: [usize; 2], text: &str) -> Result<String> {
    let mut boundaries = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(byte, _)| byte + 1))
        .collect::<Vec<_>>();
    if boundaries.last() != Some(&source.len()) {
        boundaries.push(source.len());
    }
    let span = first
        .checked_sub(1)
        .and_then(|start| {
            let end = start.checked_add(count)?;
            Some(*boundaries.get(start)?..*boundaries.get(end)?)
        })
        .ok_or_else(|| fail("script line range is outside the source"))?;

    let text = text.replace("\r\n", "\n");
    let mut edited = String::with_capacity(source.len() - span.len() + text.len() + 2);
    let prefix = &source[..span.start];
    let suffix = &source[span.end..];
    edited.push_str(prefix);
    if !text.is_empty() {
        if !prefix.is_empty() && !prefix.ends_with('\n') {
            edited.push('\n');
        }
        edited.push_str(&text);
        if !suffix.is_empty() && !text.ends_with('\n') {
            edited.push('\n');
        }
    }
    edited.push_str(suffix);
    edited.truncate(edited.trim_end_matches('\n').len());
    Ok(edited)
}
