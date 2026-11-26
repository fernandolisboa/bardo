//! Visual styles (issue #259): how a video's pictures are rendered, as a
//! photograph, a cartoon, a manga drawing, a clay model and so on.
//!
//! A channel has a default and a video project may pick another, as with
//! the format and the production mode. The catalog is data: a style is a
//! stable code and the fixed English text an image model draws it from.
//! Bardo adds that text to every image request of the video (scenes, cast
//! portraits, the thumbnail's background), so the look does not depend on
//! a planner repeating it prompt after prompt.
//!
//! Styles are described by technique (medium, line, light, texture), never
//! by naming a studio, a brand, a franchise or an artist.

use std::fmt;

/// A way of rendering a video's pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VisualStyle {
    Photorealistic,
    CinematicFilm,
    DocumentaryPhoto,
    VintageFilm,
    FilmNoir,
    Animation3d,
    Cartoon2d,
    Anime,
    Manga,
    ComicBook,
    Storybook,
    Watercolor,
    OilPainting,
    PencilSketch,
    InkDrawing,
    FlatVector,
    PaperCutout,
    ClayStopMotion,
    PixelArt,
    LowPoly,
    Isometric,
    NeonCyberpunk,
    Synthwave,
    DarkFantasy,
    WoodblockPrint,
    Chalkboard,
}

impl VisualStyle {
    /// In the order pickers list them: photographs, animation and comics,
    /// traditional media, graphic and handmade looks, then moods.
    pub const ALL: [VisualStyle; 26] = [
        VisualStyle::Photorealistic,
        VisualStyle::CinematicFilm,
        VisualStyle::DocumentaryPhoto,
        VisualStyle::VintageFilm,
        VisualStyle::FilmNoir,
        VisualStyle::Animation3d,
        VisualStyle::Cartoon2d,
        VisualStyle::Anime,
        VisualStyle::Manga,
        VisualStyle::ComicBook,
        VisualStyle::Storybook,
        VisualStyle::Watercolor,
        VisualStyle::OilPainting,
        VisualStyle::PencilSketch,
        VisualStyle::InkDrawing,
        VisualStyle::FlatVector,
        VisualStyle::PaperCutout,
        VisualStyle::ClayStopMotion,
        VisualStyle::PixelArt,
        VisualStyle::LowPoly,
        VisualStyle::Isometric,
        VisualStyle::NeonCyberpunk,
        VisualStyle::Synthwave,
        VisualStyle::DarkFantasy,
        VisualStyle::WoodblockPrint,
        VisualStyle::Chalkboard,
    ];

    /// Whether its pictures read as photographs or film: a realistic
    /// picture of a real thing drawn in it was not filmed.
    pub fn is_photographic(self) -> bool {
        matches!(
            self,
            VisualStyle::Photorealistic
                | VisualStyle::CinematicFilm
                | VisualStyle::DocumentaryPhoto
                | VisualStyle::VintageFilm
                | VisualStyle::FilmNoir
        )
    }

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            VisualStyle::Photorealistic => "photo",
            VisualStyle::CinematicFilm => "cinematic",
            VisualStyle::DocumentaryPhoto => "documentary",
            VisualStyle::VintageFilm => "vintage-film",
            VisualStyle::FilmNoir => "noir",
            VisualStyle::Animation3d => "3d-animation",
            VisualStyle::Cartoon2d => "cartoon",
            VisualStyle::Anime => "anime",
            VisualStyle::Manga => "manga",
            VisualStyle::ComicBook => "comic",
            VisualStyle::Storybook => "storybook",
            VisualStyle::Watercolor => "watercolor",
            VisualStyle::OilPainting => "oil",
            VisualStyle::PencilSketch => "pencil",
            VisualStyle::InkDrawing => "ink",
            VisualStyle::FlatVector => "flat-vector",
            VisualStyle::PaperCutout => "paper-cutout",
            VisualStyle::ClayStopMotion => "clay",
            VisualStyle::PixelArt => "pixel-art",
            VisualStyle::LowPoly => "low-poly",
            VisualStyle::Isometric => "isometric",
            VisualStyle::NeonCyberpunk => "cyberpunk",
            VisualStyle::Synthwave => "synthwave",
            VisualStyle::DarkFantasy => "dark-fantasy",
            VisualStyle::WoodblockPrint => "woodblock",
            VisualStyle::Chalkboard => "chalkboard",
        }
    }

    /// The fixed English text an image model draws it from: the words that
    /// name it, a colon, then how it is rendered. What follows the colon
    /// may be reworded from one version to the next; what comes before it
    /// may not (see [`VisualStyle::opening`]).
    pub fn prompt(self) -> &'static str {
        match self {
            VisualStyle::Photorealistic => {
                "Photorealistic photograph: sharp focus on the subject, true-to-life colors, \
                 realistic skin and materials, fine detail, light as it would fall in the \
                 real place."
            }
            VisualStyle::CinematicFilm => {
                "Cinematic film still: dramatic motivated lighting, shallow depth of field, a \
                 rich color grade with deep shadows, subtle film grain."
            }
            VisualStyle::DocumentaryPhoto => {
                "Documentary photograph: a candid, unposed moment in available light, deep \
                 focus from a wide reportage lens, natural muted colors, visible grain, \
                 slightly imperfect framing."
            }
            VisualStyle::VintageFilm => {
                "Vintage film photograph: faded warm colors, visible grain, soft focus, a \
                 gentle vignette and the slight color shift of old film stock."
            }
            VisualStyle::FilmNoir => {
                "Film noir still: high-contrast black and white, hard low-key lighting with \
                 long shadows, deep blacks, fine film grain, no color."
            }
            VisualStyle::Animation3d => {
                "3D animated film still: stylized rounded shapes with smooth surfaces, soft \
                 global illumination, rich saturated colors, clean detailed rendering."
            }
            VisualStyle::Cartoon2d => {
                "2D cartoon: bold clean outlines, flat bright colors, simple shapes, \
                 exaggerated expressions, minimal shading."
            }
            VisualStyle::Anime => {
                "Anime illustration: clean line art, cel shading with crisp highlights, \
                 expressive eyes, detailed painted backgrounds, vivid colors."
            }
            VisualStyle::Manga => {
                "Manga illustration: one full-frame drawing in black ink line art on white, \
                 screentone shading, hatching, high contrast, no color, no panels or speech \
                 bubbles."
            }
            VisualStyle::ComicBook => {
                "Comic book illustration: one full-frame drawing with bold inked outlines, \
                 halftone dot shading, strong saturated colors, no panels, speech bubbles or \
                 captions."
            }
            VisualStyle::Storybook => {
                "Storybook illustration: soft hand-painted gouache textures, warm \
                 gentle colors, rounded friendly shapes, cozy light."
            }
            VisualStyle::Watercolor => {
                "Watercolor painting: loose translucent washes, soft bleeding edges, visible \
                 paper texture, light pencil lines, an airy pale palette."
            }
            VisualStyle::OilPainting => {
                "Oil painting: visible directional brushstrokes, thick impasto in the lights, \
                 rich deep colors, edges lost into shadow, strong light and shadow."
            }
            VisualStyle::PencilSketch => {
                "Pencil sketch: graphite line drawing with an off-white paper texture, \
                 cross-hatched shading, visible construction lines, monochrome."
            }
            VisualStyle::InkDrawing => {
                "Ink drawing: black pen and brush on a white ground, confident lines of \
                 varied weight, stippling and hatching for tone, no color."
            }
            VisualStyle::FlatVector => {
                "Flat vector illustration: simple geometric shapes, solid colors from a \
                 limited palette, no outlines, no gradients or textures, a clean modern look."
            }
            VisualStyle::PaperCutout => {
                "Paper cut-out collage: layered sheets of colored paper with visible cut \
                 edges, soft shadows between the layers, handmade texture, simple shapes."
            }
            VisualStyle::ClayStopMotion => {
                "Clay stop-motion still: handmade modeling-clay figures and miniature sets, \
                 visible fingerprints and tool marks, soft studio lighting, shallow depth of \
                 field."
            }
            VisualStyle::PixelArt => {
                "Pixel art: a low-resolution 16-bit look, crisp square pixels, a limited color \
                 palette, dithering for shading, no anti-aliasing."
            }
            VisualStyle::LowPoly => {
                "Low-poly 3D render: faceted geometry of flat-shaded triangles, simple forms, \
                 clean soft lighting, pastel colors."
            }
            VisualStyle::Isometric => {
                "Isometric illustration: seen from above at a fixed isometric angle in \
                 parallel projection with no vanishing points, the scene filling the frame to \
                 its edges, tidy simplified details, soft even light."
            }
            VisualStyle::NeonCyberpunk => {
                "Neon cyberpunk digital painting: saturated magenta and cyan rim light against \
                 deep blue-black shadows, glossy reflective surfaces, light bloom and \
                 atmospheric haze, high contrast."
            }
            VisualStyle::Synthwave => {
                "Synthwave retro-futurist illustration: a 1980s airbrushed look, a palette of \
                 magenta, purple and orange gradients, glowing neon edge lines, soft bloom \
                 and haze."
            }
            VisualStyle::DarkFantasy => {
                "Dark fantasy digital painting: a desaturated palette of cold greys and earth \
                 tones, strong rim light against deep shadow, intricate detail, visible \
                 painterly brushwork, a brooding mood."
            }
            VisualStyle::WoodblockPrint => {
                "Woodblock print: flat areas of muted color, bold black contour lines, visible \
                 wood grain and paper texture, stylized shapes, no gradients, no seals or \
                 signatures."
            }
            VisualStyle::Chalkboard => {
                "Chalkboard drawing: white and colored chalk lines on dark slate that fills \
                 the whole frame, hand-drawn figures, smudges and chalk dust, no writing, \
                 numbers or diagrams, no photographic elements."
            }
        }
    }

    /// The words its text opens with, up to the colon. An image's style is
    /// read back from them in the prompt it was drawn from, so they stay as
    /// they are for good: rewording one would make every image already
    /// drawn in the style count as drawn in another.
    pub fn opening(self) -> &'static str {
        let text = self.prompt();
        text.split_once(':').map_or(text, |(opening, _)| opening)
    }

    /// What Bardo adds to an image prompt drawn in this style: the style,
    /// then what holds for every style (one picture that fills the frame,
    /// with nothing written on it).
    pub fn line(self) -> String {
        format!("{STYLE_LINE} {} {FULL_FRAME}", self.prompt())
    }

    /// How its line starts in a prompt, whatever the rest of its text was
    /// when the image was drawn.
    fn mark(self) -> String {
        format!("{STYLE_LINE} {}:", self.opening())
    }
}

/// How the line a style adds to an image prompt starts. A prompt without
/// it was drawn with no style chosen, so it never changes either.
const STYLE_LINE: &str = "Render this whole image in this visual style:";

/// What every style's line ends with. Several styles are named after an
/// object (a film still, a print, a comic), and an image model asked for
/// one tends to draw the object: bars above and below, a sheet on a desk,
/// a page of panels, a caption.
const FULL_FRAME: &str = "The picture fills the whole frame edge to edge: no border, letterbox \
    bars, page or panel layout, and no text, lettering, signature or watermark.";

impl fmt::Display for VisualStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Where a video's look comes from: a style of the catalog, or the
/// channel's aesthetic notes as the planner reads them (how every video
/// was made before styles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StyleChoice {
    /// From the channel's aesthetic notes; Bardo adds nothing.
    #[default]
    FromNotes,
    Catalog(VisualStyle),
}

impl StyleChoice {
    /// The code [`StyleChoice::FromNotes`] is stored under.
    pub const NOTES_CODE: &'static str = "notes";

    /// Every choice, the notes first.
    pub fn all() -> impl Iterator<Item = StyleChoice> {
        std::iter::once(StyleChoice::FromNotes)
            .chain(VisualStyle::ALL.into_iter().map(StyleChoice::Catalog))
    }

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            StyleChoice::FromNotes => Self::NOTES_CODE,
            StyleChoice::Catalog(style) => style.code(),
        }
    }

    /// The choice stored under `code`. A code Bardo does not know (a style
    /// of a newer version, a damaged value) is the notes: the video is
    /// still made, as it was before styles.
    pub fn from_code(code: &str) -> Self {
        VisualStyle::ALL
            .into_iter()
            .find(|style| style.code() == code)
            .map_or(StyleChoice::FromNotes, StyleChoice::Catalog)
    }

    /// The catalog style, if one was chosen.
    pub fn style(self) -> Option<VisualStyle> {
        match self {
            StyleChoice::FromNotes => None,
            StyleChoice::Catalog(style) => Some(style),
        }
    }

    /// What Bardo adds to an image prompt for it, if anything.
    pub fn line(self) -> Option<String> {
        self.style().map(VisualStyle::line)
    }

    /// `prompt` with the style's line after it.
    pub fn applied(self, prompt: &str) -> String {
        match self.line() {
            Some(line) => format!("{prompt}\n\n{line}"),
            None => prompt.to_owned(),
        }
    }

    /// Whether an image drawn from `prompt` (the whole prompt sent, as its
    /// generation record keeps it) was drawn in this choice: with this
    /// style's line, or with no style's line for the notes. A style's line
    /// is known by how it starts, so an image drawn before the rest of the
    /// style's text was reworded is still an image in that style.
    pub fn drew(self, prompt: &str) -> bool {
        match self.style() {
            Some(style) => prompt.contains(&style.mark()),
            None => !prompt.contains(STYLE_LINE),
        }
    }
}

impl fmt::Display for StyleChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_style_has_its_own_code_and_text() {
        let codes: HashSet<_> = VisualStyle::ALL.iter().map(|style| style.code()).collect();
        assert_eq!(codes.len(), VisualStyle::ALL.len());
        assert!(!codes.contains(StyleChoice::NOTES_CODE));
        let texts: HashSet<_> = VisualStyle::ALL
            .iter()
            .map(|style| style.prompt())
            .collect();
        assert_eq!(texts.len(), VisualStyle::ALL.len());
        for style in VisualStyle::ALL {
            let text = style.prompt();
            assert!(text.len() > 60 && text.ends_with('.'), "{style}: {text}");
            assert!(
                style
                    .code()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{style}"
            );
        }
    }

    #[test]
    fn codes_round_trip_and_unknown_ones_are_the_notes() {
        for choice in StyleChoice::all() {
            assert_eq!(StyleChoice::from_code(choice.code()), choice);
        }
        assert_eq!(StyleChoice::all().count(), 27);
        assert_eq!(StyleChoice::from_code("notes"), StyleChoice::FromNotes);
        assert_eq!(StyleChoice::from_code("holo-9000"), StyleChoice::FromNotes);
        assert_eq!(StyleChoice::from_code(""), StyleChoice::FromNotes);
        assert_eq!(StyleChoice::default(), StyleChoice::FromNotes);
    }

    #[test]
    fn a_style_is_added_after_the_prompt_and_the_notes_add_nothing() {
        let manga = StyleChoice::Catalog(VisualStyle::Manga);
        let prompt = manga.applied("A ship at dawn.");
        assert_eq!(
            prompt,
            format!(
                "A ship at dawn.\n\nRender this whole image in this visual style: {} The \
                 picture fills the whole frame edge to edge: no border, letterbox bars, page \
                 or panel layout, and no text, lettering, signature or watermark.",
                VisualStyle::Manga.prompt()
            )
        );
        assert_eq!(
            StyleChoice::FromNotes.applied("A ship at dawn."),
            "A ship at dawn."
        );
    }

    #[test]
    fn an_image_tells_which_style_it_was_drawn_in() {
        let manga = StyleChoice::Catalog(VisualStyle::Manga);
        let anime = StyleChoice::Catalog(VisualStyle::Anime);
        let notes = StyleChoice::FromNotes;
        let drawn = manga.applied("A ship at dawn.");
        assert!(manga.drew(&drawn));
        assert!(!anime.drew(&drawn));
        assert!(!notes.drew(&drawn));
        let plain = notes.applied("A ship at dawn, manga style.");
        assert!(notes.drew(&plain));
        assert!(!manga.drew(&plain));
    }

    /// An image's style is read from these words in the prompt it was drawn
    /// from. A changed entry here means every image already drawn in that
    /// style reads as drawn in another: reword what follows the colon
    /// instead.
    #[test]
    fn the_words_a_style_is_known_by_stay_as_they_are() {
        let openings: Vec<_> = VisualStyle::ALL
            .iter()
            .map(|style| style.opening())
            .collect();
        assert_eq!(
            openings,
            [
                "Photorealistic photograph",
                "Cinematic film still",
                "Documentary photograph",
                "Vintage film photograph",
                "Film noir still",
                "3D animated film still",
                "2D cartoon",
                "Anime illustration",
                "Manga illustration",
                "Comic book illustration",
                "Storybook illustration",
                "Watercolor painting",
                "Oil painting",
                "Pencil sketch",
                "Ink drawing",
                "Flat vector illustration",
                "Paper cut-out collage",
                "Clay stop-motion still",
                "Pixel art",
                "Low-poly 3D render",
                "Isometric illustration",
                "Neon cyberpunk digital painting",
                "Synthwave retro-futurist illustration",
                "Dark fantasy digital painting",
                "Woodblock print",
                "Chalkboard drawing",
            ]
        );
        assert_eq!(STYLE_LINE, "Render this whole image in this visual style:");
        for style in VisualStyle::ALL {
            assert!(
                style
                    .prompt()
                    .starts_with(&format!("{}: ", style.opening())),
                "{style}"
            );
        }
    }

    #[test]
    fn an_image_drawn_before_a_style_was_reworded_is_still_in_it() {
        let oil = StyleChoice::Catalog(VisualStyle::OilPainting);
        let earlier = "A ship at dawn.\n\nRender this whole image in this visual style: Oil \
                       painting: smooth glazes, as an earlier version put it.";
        assert!(oil.drew(earlier));
        assert!(!StyleChoice::FromNotes.drew(earlier));
        assert!(!StyleChoice::Catalog(VisualStyle::Watercolor).drew(earlier));
    }

    #[test]
    fn no_style_names_a_studio_a_brand_or_an_artist() {
        // Styles are techniques. This list is not complete; it catches the
        // names such prompts most often lean on.
        let names = [
            "pixar",
            "disney",
            "ghibli",
            "dreamworks",
            "marvel",
            "dc comics",
            "lego",
            "nintendo",
            "simpsons",
            "van gogh",
            "hokusai",
            "miyazaki",
            "aardman",
            "blade runner",
        ];
        for style in VisualStyle::ALL {
            let text = style.prompt().to_lowercase();
            for name in names {
                assert!(!text.contains(name), "{style} names {name}");
            }
        }
    }
}
