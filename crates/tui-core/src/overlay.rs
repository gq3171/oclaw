/// Overlay selection list with fuzzy filtering.
pub struct SelectList {
    title: String,
    items: Vec<SelectItem>,
    filtered: Vec<usize>,
    selected: usize,
    filter: String,
    visible: bool,
}

#[derive(Debug, Clone)]
pub struct SelectItem {
    pub label: String,
    pub description: String,
    pub value: String,
}

impl SelectList {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            items: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            filter: String::new(),
            visible: false,
        }
    }

    pub fn set_items(&mut self, items: Vec<SelectItem>) {
        self.items = items;
        self.refilter();
        self.selected = 0;
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.filter.clear();
        self.selected = 0;
        self.refilter();
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.filter.clear();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    pub fn type_char(&mut self, c: char) {
        self.filter.push(c);
        self.refilter();
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.filter.pop();
        self.refilter();
        self.selected = 0;
    }

    pub fn move_up(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = self.selected.checked_sub(1)
                .unwrap_or(self.filtered.len() - 1);
        }
    }

    pub fn move_down(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    pub fn confirm(&mut self) -> Option<SelectItem> {
        let item = self.filtered.get(self.selected)
            .and_then(|&i| self.items.get(i))
            .cloned();
        self.hide();
        item
    }

    pub fn filtered_items(&self) -> Vec<(usize, &SelectItem)> {
        self.filtered.iter()
            .enumerate()
            .filter_map(|(vi, &i)| self.items.get(i).map(|item| (vi, item)))
            .collect()
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    fn refilter(&mut self) {
        let query = self.filter.to_lowercase();
        if query.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            self.filtered = self.items.iter().enumerate()
                .filter(|(_, item)| {
                    let label = item.label.to_lowercase();
                    let desc = item.description.to_lowercase();
                    label.contains(&query) || desc.contains(&query)
                        || fuzzy_match(&label, &query)
                })
                .map(|(i, _)| i)
                .collect();
        }
    }
}

fn fuzzy_match(text: &str, pattern: &str) -> bool {
    let mut chars = pattern.chars();
    let mut current = chars.next();
    for c in text.chars() {
        if let Some(p) = current {
            if c == p {
                current = chars.next();
            }
        } else {
            return true;
        }
    }
    current.is_none()
}
