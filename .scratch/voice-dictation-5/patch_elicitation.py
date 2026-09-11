p = 'crates/agent_ui/src/conversation_view/elicitation.rs'
s = open(p, encoding='utf-8').read()


def rep(old, new, count=1):
    global s
    assert s.count(old) == count, (old, s.count(old))
    s = s.replace(old, new)


rep("""use acp_thread::{Elicitation, ElicitationEntryId, ElicitationStatus};
use agent_client_protocol::schema::v1 as acp;
use collections::{HashMap, HashSet};
use component::{Component, ComponentScope, example_group_with_title, single_example};
use editor::Editor;
use futures::channel::oneshot;
use gpui::{AnyElement, App, Div, Empty, Entity, Hsla, SharedString, Window, div};
""", """use acp_thread::{Elicitation, ElicitationEntryId, ElicitationStatus};
use agent_client_protocol::schema::v1 as acp;
use agent_settings::AgentSettings;
use collections::{HashMap, HashSet};
use component::{Component, ComponentScope, example_group_with_title, single_example};
use editor::Editor;
use futures::channel::oneshot;
use gpui::{AnyElement, AnyView, App, Div, Empty, Entity, Hsla, SharedString, Window, div};
use settings::Settings as _;
""")

# Answer Field: auto-height editor with soft wrap, like the Composer.
rep("""                acp::ElicitationPropertySchema::String(schema) => {
                    let options = single_select_options(schema);
                    if options.is_empty() {
                        let editor = cx.new(|cx| {
                            let mut editor = Editor::single_line(window, cx);
                            if let Some(default) = &schema.default {
                                editor.set_text(default.clone(), window, cx);
                            }
                            editor
                        });
                        ElicitationFieldState::Text(editor)
""", """                acp::ElicitationPropertySchema::String(schema) => {
                    let options = single_select_options(schema);
                    if options.is_empty() {
                        // Local: an Answer Field grows with its text like the
                        // Composer, so a dictated paragraph keeps its lines.
                        let max_lines = AgentSettings::get_global(cx).set_message_editor_max_lines();
                        let editor = cx.new(|cx| {
                            let mut editor = Editor::auto_height(1, max_lines, window, cx);
                            editor.set_soft_wrap();
                            editor.set_show_indent_guides(false, cx);
                            editor.set_show_horizontal_scrollbar(false, cx);
                            if let Some(default) = &schema.default {
                                editor.set_text(default.clone(), window, cx);
                            }
                            editor
                        });
                        ElicitationFieldState::Text(editor)
""")

rep("""    pub(crate) fn set_boolean(&mut self, field_name: &str, value: bool) {
        if let Some(ElicitationFieldState::Boolean(field)) = self.fields.get_mut(field_name) {
""", """    /// Local: the editor of a text field, the Answer Field dictation goes into.
    pub(crate) fn text_field(&self, field_name: &str) -> Option<Entity<Editor>> {
        match self.fields.get(field_name)? {
            ElicitationFieldState::Text(editor) => Some(editor.clone()),
            _ => None,
        }
    }

    pub(crate) fn set_boolean(&mut self, field_name: &str, value: bool) {
        if let Some(ElicitationFieldState::Boolean(field)) = self.fields.get_mut(field_name) {
""")

rep("""type MultiSelectHandler = Rc<dyn Fn(ElicitationEntryId, String, String, bool, &mut App)>;

#[derive(Clone)]
pub(crate) struct ElicitationCardHandlers {
    on_submit: RespondHandler,
    on_decline: RespondHandler,
    on_cancel: RespondHandler,
    on_dismiss_url: RespondHandler,
    on_open_url: OpenUrlHandler,
    on_boolean_change: BooleanHandler,
    on_single_select_change: SelectHandler,
    on_multi_select_change: MultiSelectHandler,
}
""", """type MultiSelectHandler = Rc<dyn Fn(ElicitationEntryId, String, String, bool, &mut App)>;
/// Local: a dictation hotkey or button in the named text field of a question.
type FieldHandler = Rc<dyn Fn(ElicitationEntryId, String, &mut Window, &mut App)>;
/// Local: Escape in a text field; returns whether a Dictation Session took it.
type FieldCancelHandler = Rc<dyn Fn(ElicitationEntryId, String, &mut Window, &mut App) -> bool>;

#[derive(Clone)]
pub(crate) struct ElicitationCardHandlers {
    on_submit: RespondHandler,
    on_decline: RespondHandler,
    on_cancel: RespondHandler,
    on_dismiss_url: RespondHandler,
    on_open_url: OpenUrlHandler,
    on_boolean_change: BooleanHandler,
    on_single_select_change: SelectHandler,
    on_multi_select_change: MultiSelectHandler,
    on_toggle_dictation: FieldHandler,
    on_cancel_dictation: FieldCancelHandler,
}

/// Local: what the card shows of the Dictation Session, if one is open.
#[derive(Clone, Default)]
pub(crate) struct AnswerFieldDictation {
    /// The Dictation Window open over one of this card's fields, by field name.
    pub window: Option<(String, AnyView)>,
    pub recording: bool,
    /// A Dictation Session runs somewhere: the other fields' buttons are off.
    pub blocked: bool,
}
""")

rep("""            on_boolean_change: Rc::new(on_boolean_change),
            on_single_select_change: Rc::new(on_single_select_change),
            on_multi_select_change: Rc::new(on_multi_select_change),
        }
    }
""", """            on_boolean_change: Rc::new(on_boolean_change),
            on_single_select_change: Rc::new(on_single_select_change),
            on_multi_select_change: Rc::new(on_multi_select_change),
            on_toggle_dictation: Rc::new(|_, _, _, _| {}),
            on_cancel_dictation: Rc::new(|_, _, _, _| false),
        }
    }

    /// Local: routes the dictation hotkey, microphone button and Escape of
    /// the card's text fields to the Dictation Session host.
    pub(crate) fn with_dictation(
        mut self,
        on_toggle_dictation: impl Fn(ElicitationEntryId, String, &mut Window, &mut App) + 'static,
        on_cancel_dictation: impl Fn(ElicitationEntryId, String, &mut Window, &mut App) -> bool
        + 'static,
    ) -> Self {
        self.on_toggle_dictation = Rc::new(on_toggle_dictation);
        self.on_cancel_dictation = Rc::new(on_cancel_dictation);
        self
    }
""")

rep("""pub(crate) struct ElicitationCard<'a> {
    entry_ix: usize,
    elicitation: &'a Elicitation,
    requester_name: SharedString,
    form_state: Option<&'a ElicitationFormState>,
    handlers: ElicitationCardHandlers,
}
""", """pub(crate) struct ElicitationCard<'a> {
    entry_ix: usize,
    elicitation: &'a Elicitation,
    requester_name: SharedString,
    form_state: Option<&'a ElicitationFormState>,
    handlers: ElicitationCardHandlers,
    dictation: AnswerFieldDictation,
}
""")

rep("""        Self {
            entry_ix,
            elicitation,
            requester_name,
            form_state,
            handlers,
        }
    }

    pub(crate) fn render(self, cx: &App) -> Div {""", """        Self {
            entry_ix,
            elicitation,
            requester_name,
            form_state,
            handlers,
            dictation: AnswerFieldDictation::default(),
        }
    }

    /// Local: shows the Dictation Session over the card's Answer Fields.
    pub(crate) fn with_dictation(mut self, dictation: AnswerFieldDictation) -> Self {
        self.dictation = dictation;
        self
    }

    pub(crate) fn render(self, cx: &App) -> Div {""")

rep("""            .child(match field {
                ElicitationFieldState::Text(editor) => div()
                    .rounded_sm()
                    .border_1()
                    .border_color(field_border_color)
                    .bg(editor_background)
                    .px_1()
                    .py_0p5()
                    .text_xs()
                    .child(editor.clone().into_any_element())
                    .into_any_element(),
""", """            .child(match field {
                ElicitationFieldState::Text(editor) => {
                    self.render_answer_field(field_name, editor, field_border_color, cx)
                }
""")

rep("""    fn render_single_select(
        &self,
        field_name: &str,
        selected_value: Option<&String>,""", """    /// Local: a text field with its microphone button, and the Dictation
    /// Window unfolded above it while a session is open over this field.
    fn render_answer_field(
        &self,
        field_name: &str,
        editor: &Entity<Editor>,
        field_border_color: Hsla,
        cx: &App,
    ) -> AnyElement {
        let editor_background = cx.theme().colors().editor_background;
        let elicitation_id = self.elicitation.id.clone();
        let dictation_window = self
            .dictation
            .window
            .as_ref()
            .filter(|(name, _)| name == field_name)
            .map(|(_, window)| window.clone());
        let button_state = if dictation_window.is_some() {
            if self.dictation.recording {
                DictationButtonState::Recording
            } else {
                DictationButtonState::Idle
            }
        } else if self.dictation.blocked {
            DictationButtonState::Disabled
        } else {
            DictationButtonState::Idle
        };
        let on_toggle_dictation = self.handlers.on_toggle_dictation.clone();
        let on_cancel_dictation = self.handlers.on_cancel_dictation.clone();
        let toggle = {
            let elicitation_id = elicitation_id.clone();
            let field_name = field_name.to_string();
            move |window: &mut Window, cx: &mut App| {
                on_toggle_dictation(elicitation_id.clone(), field_name.clone(), window, cx);
            }
        };
        let cancel = {
            let field_name = field_name.to_string();
            move |window: &mut Window, cx: &mut App| {
                on_cancel_dictation(elicitation_id.clone(), field_name.clone(), window, cx)
            }
        };
        let button_id = SharedString::from(format!(
            "elicitation-dictation-{}-{field_name}",
            self.entry_ix
        ));

        v_flex()
            .gap_1()
            .on_action({
                let toggle = toggle.clone();
                move |_: &crate::ToggleDictation, window, cx| toggle(window, cx)
            })
            // Escape reaches here only when the editor had nothing to cancel.
            .on_action(move |_: &editor::actions::Cancel, window, cx| {
                if !cancel(window, cx) {
                    cx.propagate();
                }
            })
            .children(dictation_window)
            .child(
                h_flex()
                    .items_start()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .rounded_sm()
                            .border_1()
                            .border_color(field_border_color)
                            .bg(editor_background)
                            .px_1()
                            .py_0p5()
                            .text_xs()
                            .child(editor.clone().into_any_element()),
                    )
                    .child(dictation_button(
                        button_id,
                        button_state,
                        editor.focus_handle(cx),
                        false,
                        move |_, window, cx| toggle(window, cx),
                    )),
            )
            .into_any_element()
    }

    fn render_single_select(
        &self,
        field_name: &str,
        selected_value: Option<&String>,""")

rep("""use ui::{
    Button, Checkbox, Color, Icon, IconName, IconSize, Indicator, Label, LabelSize, ToggleState,
    prelude::*,
};
""", """use ui::{
    Button, Checkbox, Color, Icon, IconName, IconSize, Indicator, Label, LabelSize, ToggleState,
    prelude::*,
};

use crate::dictation_window::{DictationButtonState, dictation_button};
""")

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print("ok")
