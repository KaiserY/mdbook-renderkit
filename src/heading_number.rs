use mdbook_renderer::book::{Chapter, SectionNumber};

const MAX_HEADING_DEPTH: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeadingNumber {
  pub(crate) level: usize,
  pub(crate) text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct HeadingNumberer {
  base: Vec<u32>,
  first_markdown_level: usize,
  counters: Vec<u32>,
  seen_heading: bool,
}

impl HeadingNumberer {
  pub(crate) fn new(chapter: &Chapter, first_markdown_level: usize) -> Option<Self> {
    let base = chapter
      .number
      .as_ref()
      .map(section_number_components)?
      .into_iter()
      .take(MAX_HEADING_DEPTH)
      .collect::<Vec<_>>();

    if base.is_empty() {
      return None;
    }

    Some(Self {
      counters: base.clone(),
      base,
      first_markdown_level: first_markdown_level.clamp(1, MAX_HEADING_DEPTH),
      seen_heading: false,
    })
  }

  pub(crate) fn next(&mut self, markdown_level: usize) -> HeadingNumber {
    let relative_depth = markdown_level
      .clamp(1, MAX_HEADING_DEPTH)
      .saturating_sub(self.first_markdown_level);
    let level = (self.base.len() + relative_depth).clamp(1, MAX_HEADING_DEPTH);

    if self.seen_heading {
      self.advance_to(level);
    } else {
      self.counters = self.base.clone();
      self.seen_heading = true;
    }

    HeadingNumber {
      level,
      text: format_section_components(&self.counters),
    }
  }

  fn advance_to(&mut self, level: usize) {
    if self.counters.len() >= level {
      self.counters.truncate(level);
      if let Some(last) = self.counters.last_mut() {
        *last += 1;
      }
      return;
    }

    while self.counters.len() < level {
      self.counters.push(1);
    }
  }
}

pub(crate) fn chapter_number_text(chapter: &Chapter) -> Option<String> {
  chapter
    .number
    .as_ref()
    .map(section_number_components)
    .map(|components| {
      components
        .into_iter()
        .take(MAX_HEADING_DEPTH)
        .collect::<Vec<_>>()
    })
    .filter(|components| !components.is_empty())
    .map(|components| format_section_components(&components))
}

pub(crate) fn chapter_level(chapter: &Chapter) -> usize {
  chapter
    .number
    .as_ref()
    .map_or(1, |number| number.len().clamp(1, MAX_HEADING_DEPTH))
}

fn section_number_components(number: &SectionNumber) -> Vec<u32> {
  number.iter().copied().collect()
}

fn format_section_components(components: &[u32]) -> String {
  if components.is_empty() {
    return "0".to_string();
  }

  let mut out = components
    .iter()
    .map(u32::to_string)
    .collect::<Vec<_>>()
    .join(".");
  out.push('.');
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  fn chapter(number: Vec<u32>) -> Chapter {
    let mut chapter = Chapter::new("chapter", String::new(), "chapter.md", Vec::new());
    chapter.number = Some(SectionNumber::new(number));
    chapter
  }

  #[test]
  fn heading_numbers_start_from_mdbook_chapter_number() {
    let chapter = chapter(vec![2, 3]);
    let mut numberer = HeadingNumberer::new(&chapter, 2).expect("numberer");

    assert_eq!(
      numberer.next(2),
      HeadingNumber {
        level: 2,
        text: "2.3.".to_string(),
      }
    );
    assert_eq!(
      numberer.next(3),
      HeadingNumber {
        level: 3,
        text: "2.3.1.".to_string(),
      }
    );
    assert_eq!(
      numberer.next(3),
      HeadingNumber {
        level: 3,
        text: "2.3.2.".to_string(),
      }
    );
    assert_eq!(
      numberer.next(4),
      HeadingNumber {
        level: 4,
        text: "2.3.2.1.".to_string(),
      }
    );
  }

  #[test]
  fn heading_numbers_clamp_to_six_levels() {
    let chapter = chapter(vec![1, 2, 3, 4, 5]);
    let mut numberer = HeadingNumberer::new(&chapter, 1).expect("numberer");

    assert_eq!(numberer.next(1).text, "1.2.3.4.5.");
    assert_eq!(numberer.next(6).level, 6);
    assert_eq!(numberer.next(6).text, "1.2.3.4.5.2.");
  }
}
