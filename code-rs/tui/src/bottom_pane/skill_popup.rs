use code_common::fuzzy_match::fuzzy_match;
use code_protocol::skills::Skill;
use ratatui::buffer::Buffer;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::render_rows;

pub(crate) struct SkillPopup {
    skills: Vec<Skill>,
    filter: String,
    state: ScrollState,
}

impl SkillPopup {
    pub(crate) fn new(skills: Vec<Skill>) -> Self {
        Self {
            skills,
            filter: String::new(),
            state: ScrollState::new(),
        }
    }

    pub(crate) fn on_composer_text_change(&mut self, filter: String) {
        self.filter = filter;
        let matches_len = self.filtered().len();
        self.state.clamp_selection(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    pub(crate) fn set_skills(&mut self, skills: Vec<Skill>) {
        self.skills = skills;
        let matches_len = self.filtered().len();
        self.state.clamp_selection(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    pub(crate) fn calculate_required_height(&self) -> u16 {
        self.filtered().len().clamp(1, MAX_POPUP_ROWS) as u16
    }

    pub(crate) fn match_count(&self) -> usize {
        self.filtered().len()
    }

    pub(crate) fn move_up(&mut self) {
        let len = self.filtered().len();
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.filtered().len();
        self.state.move_down_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    pub(crate) fn selected_skill_name(&self) -> Option<String> {
        let matches = self.filtered();
        self.state
            .selected_idx
            .and_then(|idx| matches.get(idx))
            .map(|(skill, _, _)| skill.name.clone())
    }

    fn filtered(&self) -> Vec<(&Skill, Option<Vec<usize>>, i32)> {
        let filter = self.filter.trim_start_matches('$').trim();
        let mut out = Vec::new();

        if filter.is_empty() {
            for skill in &self.skills {
                out.push((skill, None, 0));
            }
        } else {
            for skill in &self.skills {
                if let Some((indices, score)) = fuzzy_match(&skill.name, filter) {
                    out.push((skill, Some(indices), score));
                }
            }
            out.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.name.cmp(&b.0.name)));
        }

        out
    }
}

impl WidgetRef for SkillPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let indented_area = area.inner(Margin::new(2, 0));
        let rows: Vec<GenericDisplayRow> = self
            .filtered()
            .into_iter()
            .map(|(skill, indices, _)| {
                let description = if skill.allow_implicit_invocation {
                    skill
                        .short_description
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or(skill.description.as_str())
                        .to_string()
                } else {
                    let summary = skill
                        .short_description
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or(skill.description.as_str());
                    format!("[manual] {summary}")
                };

                GenericDisplayRow {
                    name: format!("${}", skill.name),
                    match_indices: indices.map(|values| values.into_iter().map(|idx| idx + 1).collect()),
                    is_current: false,
                    description: Some(description),
                    name_color: Some(crate::colors::primary()),
                }
            })
            .collect();

        render_rows(
            indented_area,
            buf,
            &rows,
            &self.state,
            MAX_POPUP_ROWS,
            false,
        );
    }
}
