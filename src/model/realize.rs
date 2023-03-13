use super::{Content, NodeId, Recipe, Selector, StyleChain, Vt};
use crate::diag::SourceResult;
use crate::doc::{Meta, MetaNode};

/// Whether the target is affected by show rules in the given style chain.
pub fn applicable(target: &Content, styles: StyleChain) -> bool {
    if target.needs_preparation() {
        return true;
    }

    if target.can::<dyn Show>() && target.is_pristine() {
        return true;
    }

    // Find out how many recipes there are.
    let mut n = styles.recipes().count();

    // Find out whether any recipe matches and is unguarded.
    for recipe in styles.recipes() {
        if recipe.applicable(target) && !target.is_guarded(Guard::Nth(n)) {
            return true;
        }
        n -= 1;
    }

    false
}

/// Apply the show rules in the given style chain to a target.
pub fn realize(
    vt: &mut Vt,
    target: &Content,
    styles: StyleChain,
) -> SourceResult<Option<Content>> {
    // Pre-process.
    if target.needs_preparation() {
        let mut node = target.clone();
        if target.can::<dyn Locatable>() || target.label().is_some() {
            let id = vt.identify(target);
            node.set_stable_id(id);
        }

        if let Some(node) = node.with_mut::<dyn Synthesize>() {
            node.synthesize(vt, styles);
        }

        node.mark_prepared();

        if let Some(id) = node.stable_id() {
            let meta = Meta::Node(id, node.clone());
            return Ok(Some(node.styled(MetaNode::set_data(vec![meta]))));
        }

        return Ok(Some(node));
    }

    // Find out how many recipes there are.
    let mut n = styles.recipes().count();

    // Find an applicable recipe.
    let mut realized = None;
    for recipe in styles.recipes() {
        let guard = Guard::Nth(n);
        if recipe.applicable(target) && !target.is_guarded(guard) {
            if let Some(content) = try_apply(vt, target, recipe, guard)? {
                realized = Some(content);
                break;
            }
        }
        n -= 1;
    }

    // Realize if there was no matching recipe.
    if let Some(showable) = target.with::<dyn Show>() {
        let guard = Guard::Base(target.id());
        if realized.is_none() && !target.is_guarded(guard) {
            realized = Some(showable.show(vt, styles)?);
        }
    }

    // Finalize only if this is the first application for this node.
    if let Some(node) = target.with::<dyn Finalize>() {
        if target.is_pristine() {
            if let Some(already) = realized {
                realized = Some(node.finalize(already, styles));
            }
        }
    }

    Ok(realized)
}

/// Try to apply a recipe to the target.
fn try_apply(
    vt: &Vt,
    target: &Content,
    recipe: &Recipe,
    guard: Guard,
) -> SourceResult<Option<Content>> {
    match &recipe.selector {
        Some(Selector::Node(id, _)) => {
            if target.id() != *id {
                return Ok(None);
            }

            recipe.apply(vt.world(), target.clone().guarded(guard)).map(Some)
        }

        Some(Selector::Label(label)) => {
            if target.label() != Some(label) {
                return Ok(None);
            }

            recipe.apply(vt.world(), target.clone().guarded(guard)).map(Some)
        }

        Some(Selector::Regex(regex)) => {
            let Some(text) = item!(text_str)(target) else {
                return Ok(None);
            };

            let make = |s| {
                let mut content = item!(text)(s);
                content.copy_modifiers(target);
                content
            };

            let mut result = vec![];
            let mut cursor = 0;

            for m in regex.find_iter(&text) {
                let start = m.start();
                if cursor < start {
                    result.push(make(text[cursor..start].into()));
                }

                let piece = make(m.as_str().into()).guarded(guard);
                let transformed = recipe.apply(vt.world(), piece)?;
                result.push(transformed);
                cursor = m.end();
            }

            if result.is_empty() {
                return Ok(None);
            }

            if cursor < text.len() {
                result.push(make(text[cursor..].into()));
            }

            Ok(Some(Content::sequence(result)))
        }

        None => Ok(None),
    }
}

/// Makes this node locatable through `vt.locate`.
pub trait Locatable {}

/// Synthesize fields on a node. This happens before execution of any show rule.
pub trait Synthesize {
    /// Prepare the node for show rule application.
    fn synthesize(&mut self, vt: &Vt, styles: StyleChain);
}

/// The base recipe for a node.
pub trait Show {
    /// Execute the base recipe for this node.
    fn show(&self, vt: &mut Vt, styles: StyleChain) -> SourceResult<Content>;
}

/// Post-process a node after it was realized.
pub trait Finalize {
    /// Finalize the fully realized form of the node. Use this for effects that
    /// should work even in the face of a user-defined show rule, for example
    /// the linking behaviour of a link node.
    fn finalize(&self, realized: Content, styles: StyleChain) -> Content;
}

/// Guards content against being affected by the same show rule multiple times.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Guard {
    /// The nth recipe from the top of the chain.
    Nth(usize),
    /// The [base recipe](Show) for a kind of node.
    Base(NodeId),
}
