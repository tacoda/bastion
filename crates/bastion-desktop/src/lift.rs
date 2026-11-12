//! Everything a person can press lifts a little under the pointer, as the
//! landmarks on the maps do, so what is clickable is plain to see.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay, renderer};
use iced::widget::button::{self as iced_button, Button};
use iced::{Element, Event, Length, Padding, Rectangle, Size, Theme, Transformation, Vector};

/// How much a pressable thing grows under the pointer.
const LIFT: f32 = 1.04;

/// `content`, drawn a little larger while the pointer is over it.
pub struct Lift<'a, M> {
    content: Element<'a, M>,
}

impl<M> Widget<M, Theme, iced::Renderer> for Lift<'_, M> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let content = self.content.as_widget_mut();
        content.operate(tree, layout, renderer, operation);
    }

    // iced's trait fixes this signature.
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let content = self.content.as_widget_mut();
        content.update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let content = self.content.as_widget();
        content.mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    // iced's trait fixes this signature.
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let content = self.content.as_widget();
        let bounds = layout.bounds();
        if !cursor.is_over(bounds) {
            return content.draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        let middle = bounds.center();
        let grow = Transformation::translate(middle.x, middle.y)
            * Transformation::scale(LIFT)
            * Transformation::translate(-middle.x, -middle.y);
        use iced::advanced::Renderer as _;
        renderer.with_transformation(grow, |r| {
            content.draw(tree, r, theme, style, layout, cursor, viewport);
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, iced::Renderer>> {
        let content = self.content.as_widget_mut();
        content.overlay(tree, layout, renderer, viewport, translation)
    }
}

impl<'a, M: 'a> From<Lift<'a, M>> for Element<'a, M> {
    fn from(lift: Lift<'a, M>) -> Self {
        Element::new(lift)
    }
}

/// A button that lifts under the pointer: iced's button, with the same
/// builder methods, wrapped in [`Lift`].
pub struct Pressable<'a, M>(Button<'a, M>);

/// A button that lifts under the pointer.
pub fn button<'a, M>(content: impl Into<Element<'a, M>>) -> Pressable<'a, M> {
    Pressable(iced::widget::button(content))
}

impl<'a, M: Clone + 'a> Pressable<'a, M> {
    pub fn on_press(self, msg: M) -> Self {
        Pressable(self.0.on_press(msg))
    }

    pub fn on_press_maybe(self, msg: Option<M>) -> Self {
        Pressable(self.0.on_press_maybe(msg))
    }

    pub fn padding(self, padding: impl Into<Padding>) -> Self {
        Pressable(self.0.padding(padding))
    }

    pub fn width(self, width: impl Into<Length>) -> Self {
        Pressable(self.0.width(width))
    }

    pub fn height(self, height: impl Into<Length>) -> Self {
        Pressable(self.0.height(height))
    }

    pub fn style(
        self,
        style: impl Fn(&Theme, iced_button::Status) -> iced_button::Style + 'a,
    ) -> Self {
        Pressable(self.0.style(style))
    }
}

impl<'a, M: Clone + 'a> From<Pressable<'a, M>> for Element<'a, M> {
    fn from(p: Pressable<'a, M>) -> Self {
        Element::new(Lift {
            content: p.0.into(),
        })
    }
}
