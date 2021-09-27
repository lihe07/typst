use super::*;
use crate::layout::{FixedNode, GridNode, PadNode, StackChild, StackNode, TrackSizing};
use crate::paper::{Paper, PaperClass};

/// `page`: Configure pages.
pub fn page(ctx: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let paper = match args.eat::<Spanned<Str>>() {
        Some(name) => match Paper::from_name(&name.v) {
            None => bail!(name.span, "invalid paper name"),
            paper => paper,
        },
        None => None,
    };

    let width = args.named("width")?;
    let height = args.named("height")?;
    let margins = args.named("margins")?;
    let left = args.named("left")?;
    let top = args.named("top")?;
    let right = args.named("right")?;
    let bottom = args.named("bottom")?;
    let flip = args.named("flip")?;

    ctx.template.modify(move |state| {
        let page = state.page_mut();

        if let Some(paper) = paper {
            page.class = paper.class();
            page.size = paper.size();
        }

        if let Some(width) = width {
            page.class = PaperClass::Custom;
            page.size.w = width;
        }

        if let Some(height) = height {
            page.class = PaperClass::Custom;
            page.size.h = height;
        }

        if let Some(margins) = margins {
            page.margins = Sides::splat(Some(margins));
        }

        if let Some(left) = left {
            page.margins.left = Some(left);
        }

        if let Some(top) = top {
            page.margins.top = Some(top);
        }

        if let Some(right) = right {
            page.margins.right = Some(right);
        }

        if let Some(bottom) = bottom {
            page.margins.bottom = Some(bottom);
        }

        if flip.unwrap_or(false) {
            std::mem::swap(&mut page.size.w, &mut page.size.h);
        }
    });

    ctx.template.pagebreak(false);

    Ok(Value::None)
}

/// `pagebreak`: Start a new page.
pub fn pagebreak(ctx: &mut EvalContext, _: &mut Args) -> TypResult<Value> {
    ctx.template.pagebreak(true);
    Ok(Value::None)
}

/// `h`: Horizontal spacing.
pub fn h(ctx: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let spacing = args.expect("spacing")?;
    ctx.template.spacing(GenAxis::Inline, spacing);
    Ok(Value::None)
}

/// `v`: Vertical spacing.
pub fn v(ctx: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let spacing = args.expect("spacing")?;
    ctx.template.spacing(GenAxis::Block, spacing);
    Ok(Value::None)
}

/// `align`: Configure the alignment along the layouting axes.
pub fn align(ctx: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let first = args.eat::<Align>();
    let second = args.eat::<Align>();
    let body = args.eat::<Template>();

    let mut horizontal = args.named("horizontal")?;
    let mut vertical = args.named("vertical")?;

    for value in first.into_iter().chain(second) {
        match value.axis() {
            Some(SpecAxis::Horizontal) | None if horizontal.is_none() => {
                horizontal = Some(value);
            }
            Some(SpecAxis::Vertical) | None if vertical.is_none() => {
                vertical = Some(value);
            }
            _ => {}
        }
    }

    let realign = |template: &mut Template| {
        template.modify(move |state| {
            if let Some(horizontal) = horizontal {
                state.aligns.inline = horizontal;
            }

            if let Some(vertical) = vertical {
                state.aligns.block = vertical;
            }
        });

        if vertical.is_some() {
            template.parbreak();
        }
    };

    Ok(if let Some(body) = body {
        let mut template = Template::new();
        template.save();
        realign(&mut template);
        template += body;
        template.restore();
        Value::Template(template)
    } else {
        realign(&mut ctx.template);
        Value::None
    })
}

/// `box`: Place content in a rectangular box.
pub fn boxed(_: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let width = args.named("width")?;
    let height = args.named("height")?;
    let body: Template = args.eat().unwrap_or_default();
    Ok(Value::Template(Template::from_inline(move |state| {
        FixedNode {
            width,
            height,
            aspect: None,
            child: body.to_stack(state).into(),
        }
    })))
}

/// `block`: Place content in a block.
pub fn block(_: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let body: Template = args.expect("body")?;
    Ok(Value::Template(Template::from_block(move |state| {
        body.to_stack(state)
    })))
}

/// `pad`: Pad content at the sides.
pub fn pad(_: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    let all = args.eat();
    let left = args.named("left")?;
    let top = args.named("top")?;
    let right = args.named("right")?;
    let bottom = args.named("bottom")?;
    let body: Template = args.expect("body")?;

    let padding = Sides::new(
        left.or(all).unwrap_or_default(),
        top.or(all).unwrap_or_default(),
        right.or(all).unwrap_or_default(),
        bottom.or(all).unwrap_or_default(),
    );

    Ok(Value::Template(Template::from_block(move |state| {
        PadNode {
            padding,
            child: body.to_stack(&state).into(),
        }
    })))
}

/// `stack`: Stack children along an axis.
pub fn stack(_: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    enum Child {
        Spacing(Linear),
        Any(Template),
    }

    castable! {
        Child: "linear or template",
        Value::Length(v) => Self::Spacing(v.into()),
        Value::Relative(v) => Self::Spacing(v.into()),
        Value::Linear(v) => Self::Spacing(v),
        Value::Template(v) => Self::Any(v),
    }

    let dir = args.named("dir")?;
    let spacing = args.named("spacing")?;
    let list: Vec<Child> = args.all().collect();

    Ok(Value::Template(Template::from_block(move |state| {
        let mut dirs = Gen::new(None, dir).unwrap_or(state.dirs);

        // If the directions become aligned, fix up the inline direction since
        // that's the one that is not user-defined.
        if dirs.block.axis() == dirs.inline.axis() {
            dirs.inline = state.dirs.block;
        }

        // Use the current alignments for all children, but take care to apply
        // them to the correct axes (by swapping them if the stack axes are
        // different from the state axes).
        let mut aligns = state.aligns;
        if dirs.block.axis() == state.dirs.inline.axis() {
            aligns = Gen::new(aligns.block, aligns.inline);
        }

        let mut children = vec![];
        let mut delayed = None;

        // Build the list of stack children.
        for child in &list {
            match child {
                Child::Spacing(v) => {
                    children.push(StackChild::Spacing(*v));
                    delayed = None;
                }
                Child::Any(template) => {
                    if let Some(v) = delayed {
                        children.push(StackChild::Spacing(v));
                    }

                    let node = template.to_stack(state).into();
                    children.push(StackChild::Any(node, aligns));
                    delayed = spacing;
                }
            }
        }

        StackNode { dirs, children }
    })))
}

/// `grid`: Arrange children into a grid.
pub fn grid(_: &mut EvalContext, args: &mut Args) -> TypResult<Value> {
    castable! {
        Vec<TrackSizing>: "array of autos, linears, and fractionals",
        Value::Int(count) => vec![TrackSizing::Auto; count.max(0) as usize],
        Value::Array(values) => values
            .into_iter()
            .filter_map(|v| v.cast().ok())
            .collect(),
    }

    castable! {
        TrackSizing: "auto, linear, or fractional",
        Value::Auto => Self::Auto,
        Value::Length(v) => Self::Linear(v.into()),
        Value::Relative(v) => Self::Linear(v.into()),
        Value::Linear(v) => Self::Linear(v),
        Value::Fractional(v) => Self::Fractional(v),
    }

    let columns = args.named("columns")?.unwrap_or_default();
    let rows = args.named("rows")?.unwrap_or_default();
    let tracks = Gen::new(columns, rows);

    let column_dir = args.named("column-dir")?;
    let row_dir = args.named("row-dir")?;

    let gutter_columns = args.named("gutter-columns")?;
    let gutter_rows = args.named("gutter-rows")?;
    let gutter_default = args
        .named("gutter")?
        .map(|v| vec![TrackSizing::Linear(v)])
        .unwrap_or_default();

    let gutter = Gen::new(
        gutter_columns.unwrap_or_else(|| gutter_default.clone()),
        gutter_rows.unwrap_or(gutter_default),
    );

    let children: Vec<Template> = args.all().collect();

    Ok(Value::Template(Template::from_block(move |state| {
        // If the directions become aligned, try to fix up the direction which
        // is not user-defined.
        let mut dirs = Gen::new(column_dir, row_dir).unwrap_or(state.dirs);
        if dirs.block.axis() == dirs.inline.axis() {
            let target = if column_dir.is_some() {
                &mut dirs.block
            } else {
                &mut dirs.inline
            };

            *target = if target.axis() == state.dirs.inline.axis() {
                state.dirs.block
            } else {
                state.dirs.inline
            };
        }

        let children =
            children.iter().map(|child| child.to_stack(&state).into()).collect();

        GridNode {
            dirs,
            tracks: tracks.clone(),
            gutter: gutter.clone(),
            children,
        }
    })))
}
