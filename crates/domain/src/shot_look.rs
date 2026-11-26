//! How a product shot looks (#285, ADR-0041): how the product's image is
//! set (a card, a cut-out photo standing on the backdrop, or a screen in
//! a plain device outline), where it sits, and what is behind it (a tone
//! of the image, a color or gradient, or a backdrop drawn for the
//! project). Whatever the look, the image is only ever scaled: never
//! stretched, cut or redrawn.

use std::time::SystemTime;

use crate::{JobId, ProductImageKind, Provider, VideoProjectId};

uuid_id!(
    /// Identifies a backdrop drawn for a project's product shots.
    BackdropId
);

/// How a product's image is set in its shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductLayout {
    /// The image as a card with rounded corners.
    Card,
    /// A photo with a see-through background standing on the backdrop,
    /// with a soft shadow.
    Cutout,
    /// A screen inside a plain outline Bardo draws: a phone for a tall
    /// screen, a browser window for a wide one. No maker's marks.
    Device,
}

impl ProductLayout {
    pub const ALL: [ProductLayout; 3] = [
        ProductLayout::Card,
        ProductLayout::Cutout,
        ProductLayout::Device,
    ];

    pub fn code(self) -> &'static str {
        match self {
            ProductLayout::Card => "card",
            ProductLayout::Cutout => "cutout",
            ProductLayout::Device => "device",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layout| layout.code() == code)
    }

    /// The layouts an image of `kind` may be shown in, the one it gets
    /// when none is chosen first. Only a photo marked cut out stands on
    /// the backdrop; a screen goes in a device; every image may be a card.
    pub fn offered(kind: ProductImageKind, cut_out: bool) -> Vec<ProductLayout> {
        match kind {
            ProductImageKind::Photo if cut_out => {
                vec![ProductLayout::Cutout, ProductLayout::Card]
            }
            ProductImageKind::Photo => vec![ProductLayout::Card],
            ProductImageKind::Screen => vec![ProductLayout::Device, ProductLayout::Card],
        }
    }
}

/// The outline a screen is set in, from its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// A tall screen: a phone.
    Phone,
    /// A wide or square screen: a browser window.
    Browser,
}

impl DeviceKind {
    /// The device a screen of `picture` pixels (width, height) is shown in.
    pub fn of(picture: (u32, u32)) -> Self {
        if picture.1 > picture.0 {
            DeviceKind::Phone
        } else {
            DeviceKind::Browser
        }
    }
}

/// Where the product sits across the room its shot leaves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShotPlacement {
    #[default]
    Center,
    Left,
    Right,
}

impl ShotPlacement {
    pub const ALL: [ShotPlacement; 3] = [
        ShotPlacement::Left,
        ShotPlacement::Center,
        ShotPlacement::Right,
    ];

    pub fn code(self) -> &'static str {
        match self {
            ShotPlacement::Center => "center",
            ShotPlacement::Left => "left",
            ShotPlacement::Right => "right",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|place| place.code() == code)
    }
}

/// Which way a backdrop's gradient runs, from its first color to its
/// second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BackdropDirection {
    /// Top to bottom.
    #[default]
    Down,
    /// Left to right.
    Across,
    /// Top left to bottom right.
    Diagonal,
}

impl BackdropDirection {
    pub const ALL: [BackdropDirection; 3] = [
        BackdropDirection::Down,
        BackdropDirection::Across,
        BackdropDirection::Diagonal,
    ];

    pub fn code(self) -> &'static str {
        match self {
            BackdropDirection::Down => "down",
            BackdropDirection::Across => "across",
            BackdropDirection::Diagonal => "diagonal",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|way| way.code() == code)
    }
}

/// A color of a backdrop: red, green and blue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShotColor(pub [u8; 3]);

impl ShotColor {
    pub const WHITE: ShotColor = ShotColor([250, 250, 248]);
    pub const BLACK: ShotColor = ShotColor([20, 20, 23]);

    /// Six hex digits, as `1a2b3c`.
    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("{r:02x}{g:02x}{b:02x}")
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        Some(ShotColor([channel(0)?, channel(2)?, channel(4)?]))
    }

    /// Colors as stored: hex, separated by commas. One that will not read
    /// is left out.
    pub fn list_code(colors: &[ShotColor]) -> String {
        colors
            .iter()
            .map(|color| color.hex())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn list_from_code(code: &str) -> Vec<ShotColor> {
        code.split(',').filter_map(ShotColor::from_hex).collect()
    }
}

/// What is behind the product in its shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShotBackdrop {
    /// A plain tone of the image's own color, deep under a light image
    /// and pale under a dark one.
    #[default]
    Auto,
    /// A color, or a gradient between two.
    Plain {
        from: ShotColor,
        to: ShotColor,
        direction: BackdropDirection,
    },
    /// A backdrop drawn for the project and kept by the user.
    Drawn(BackdropId),
}

impl ShotBackdrop {
    /// A single color.
    pub fn color(color: ShotColor) -> Self {
        ShotBackdrop::Plain {
            from: color,
            to: color,
            direction: BackdropDirection::Down,
        }
    }

    fn code(self) -> String {
        match self {
            ShotBackdrop::Auto => "auto".into(),
            ShotBackdrop::Plain {
                from,
                to,
                direction,
            } => format!("plain-{}-{}-{}", from.hex(), to.hex(), direction.code()),
            ShotBackdrop::Drawn(id) => format!("drawn-{id}"),
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        if code == "auto" {
            return Some(ShotBackdrop::Auto);
        }
        if let Some(id) = code.strip_prefix("drawn-") {
            return Some(ShotBackdrop::Drawn(BackdropId::from(
                uuid::Uuid::parse_str(id).ok()?,
            )));
        }
        let mut parts = code.strip_prefix("plain-")?.split('-');
        let backdrop = ShotBackdrop::Plain {
            from: ShotColor::from_hex(parts.next()?)?,
            to: ShotColor::from_hex(parts.next()?)?,
            direction: BackdropDirection::from_code(parts.next()?)?,
        };
        parts.next().is_none().then_some(backdrop)
    }
}

/// How one product shot looks. The default is what a shot gets before
/// the user changes anything: the layout its image is first offered in,
/// in the middle, over a tone of the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ShotLook {
    /// `None` for the layout the image is first offered in
    /// ([`ProductLayout::offered`]).
    pub layout: Option<ProductLayout>,
    pub placement: ShotPlacement,
    pub backdrop: ShotBackdrop,
}

impl ShotLook {
    /// The layout an image of `kind` is shown in under this look: the one
    /// chosen when the image offers it, else the image's first.
    pub fn layout_for(&self, kind: ProductImageKind, cut_out: bool) -> ProductLayout {
        let offered = ProductLayout::offered(kind, cut_out);
        self.layout
            .filter(|layout| offered.contains(layout))
            .unwrap_or(offered[0])
    }

    /// Stored as one line: layout, place and backdrop, separated by `|`.
    pub fn code(&self) -> String {
        format!(
            "{}|{}|{}",
            self.layout.map_or("default", ProductLayout::code),
            self.placement.code(),
            self.backdrop.code()
        )
    }

    /// The look a stored line names; `None` when it will not read.
    pub fn from_code(code: &str) -> Option<Self> {
        let mut parts = code.trim().split('|');
        let layout = match parts.next()? {
            "default" => None,
            layout => Some(ProductLayout::from_code(layout)?),
        };
        let look = ShotLook {
            layout,
            placement: ShotPlacement::from_code(parts.next()?)?,
            backdrop: ShotBackdrop::from_code(parts.next()?)?,
        };
        parts.next().is_none().then_some(look)
    }

    /// The drawn backdrop this look shows, when it shows one.
    pub fn drawn_backdrop(&self) -> Option<BackdropId> {
        match self.backdrop {
            ShotBackdrop::Drawn(id) => Some(id),
            _ => None,
        }
    }
}

/// A backdrop the image generator drew for a project's product shots:
/// an empty surface or set in the video's style, with no product, text or
/// logo. It waits for the user's review; once kept, every composed scene
/// of the project may show it, at no cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotBackdropDrawing {
    pub id: BackdropId,
    pub project: VideoProjectId,
    /// The image file in the project folder.
    pub file: String,
    pub prompt: String,
    pub provider: Provider,
    pub model: String,
    /// The job that drew it.
    pub job: Option<JobId>,
    pub drawn_at: SystemTime,
    /// The user kept it after looking at it.
    pub kept: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_image_is_offered_the_layouts_that_show_it_whole() {
        assert_eq!(
            ProductLayout::offered(ProductImageKind::Photo, true),
            [ProductLayout::Cutout, ProductLayout::Card]
        );
        assert_eq!(
            ProductLayout::offered(ProductImageKind::Photo, false),
            [ProductLayout::Card]
        );
        assert_eq!(
            ProductLayout::offered(ProductImageKind::Screen, false),
            [ProductLayout::Device, ProductLayout::Card]
        );
        assert_eq!(DeviceKind::of((1080, 2340)), DeviceKind::Phone);
        assert_eq!(DeviceKind::of((1920, 1080)), DeviceKind::Browser);
        assert_eq!(DeviceKind::of((800, 800)), DeviceKind::Browser);
    }

    #[test]
    fn a_layout_the_image_does_not_offer_falls_back_to_its_first() {
        let look = ShotLook {
            layout: Some(ProductLayout::Cutout),
            ..ShotLook::default()
        };
        assert_eq!(
            look.layout_for(ProductImageKind::Photo, true),
            ProductLayout::Cutout
        );
        assert_eq!(
            look.layout_for(ProductImageKind::Photo, false),
            ProductLayout::Card
        );
        assert_eq!(
            look.layout_for(ProductImageKind::Screen, false),
            ProductLayout::Device
        );
        assert_eq!(
            ShotLook::default().layout_for(ProductImageKind::Screen, false),
            ProductLayout::Device
        );
    }

    #[test]
    fn looks_keep_their_codes() {
        let looks = [
            ShotLook::default(),
            ShotLook {
                layout: Some(ProductLayout::Device),
                placement: ShotPlacement::Left,
                backdrop: ShotBackdrop::Plain {
                    from: ShotColor([1, 2, 3]),
                    to: ShotColor([250, 128, 0]),
                    direction: BackdropDirection::Diagonal,
                },
            },
            ShotLook {
                layout: Some(ProductLayout::Card),
                placement: ShotPlacement::Right,
                backdrop: ShotBackdrop::Drawn(BackdropId::new()),
            },
        ];
        for look in looks {
            assert_eq!(ShotLook::from_code(&look.code()), Some(look), "{look:?}");
        }
        assert_eq!(ShotLook::default().code(), "default|center|auto");
        for broken in [
            "",
            "default|center",
            "round|center|auto",
            "default|up|auto",
            "default|center|plain-zzzzzz-000000-down",
            "default|center|plain-000000-000000-down-x",
            "default|center|drawn-nope",
            "default|center|auto|more",
        ] {
            assert_eq!(ShotLook::from_code(broken), None, "{broken}");
        }
    }

    #[test]
    fn colors_read_as_hex() {
        assert_eq!(ShotColor([255, 0, 16]).hex(), "ff0010");
        assert_eq!(
            ShotColor::from_hex("#FF0010"),
            Some(ShotColor([255, 0, 16]))
        );
        assert_eq!(ShotColor::from_hex("ff001"), None);
        assert_eq!(ShotColor::from_hex("ééé"), None);
        let colors = [ShotColor([1, 2, 3]), ShotColor::WHITE];
        assert_eq!(
            ShotColor::list_from_code(&ShotColor::list_code(&colors)),
            colors
        );
        assert_eq!(ShotColor::list_from_code(""), []);
    }
}
