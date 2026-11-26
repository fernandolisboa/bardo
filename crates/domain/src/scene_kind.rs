//! What a scene of an ad shows and how its picture is made (#278,
//! ADR-0040): drawn by the image generator as every other scene, or
//! composed by Bardo from one of the product's own images. Scenes of a
//! video that is no ad have no kind: they are drawn scenes.

use crate::{AspectRatio, ProductImage, ProductImageId, ProductImageKind, ShotColor, ShotLook};

/// What one scene of an ad is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SceneKind {
    /// A picture the image generator draws from the scene's prompt.
    #[default]
    Drawn,
    /// The product being used, drawn. Only for a product with a photo.
    ProductInUse,
    /// The product's own image, composed by Bardo over a backdrop. Only
    /// for a product with an image.
    ProductShot,
    /// The ad's last picture, over the call to action.
    EndCard,
}

impl SceneKind {
    pub const ALL: [SceneKind; 4] = [
        SceneKind::Drawn,
        SceneKind::ProductInUse,
        SceneKind::ProductShot,
        SceneKind::EndCard,
    ];

    /// Stable name: stored in the database and asked of the planner.
    pub fn code(self) -> &'static str {
        match self {
            SceneKind::Drawn => "drawn",
            SceneKind::ProductInUse => "product_in_use",
            SceneKind::ProductShot => "product_shot",
            SceneKind::EndCard => "end_card",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.code() == code.trim())
    }
}

/// The images of an ad's product as its scenes may use them: each one's
/// id and what it shows, in the product's order. A product with no image
/// has none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductPictures(Vec<(ProductImageId, ProductImageKind)>);

impl ProductPictures {
    pub fn new(images: Vec<(ProductImageId, ProductImageKind)>) -> Self {
        Self(images)
    }

    pub fn of(images: &[ProductImage]) -> Self {
        Self(images.iter().map(|image| (image.id, image.kind)).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Each image's id and what it shows, in the product's order.
    pub fn images(&self) -> &[(ProductImageId, ProductImageKind)] {
        &self.0
    }

    /// The product's first image: what a product shot shows when no
    /// other is named.
    pub fn first(&self) -> Option<ProductImageId> {
        self.0.first().map(|(id, _)| *id)
    }

    /// The image at `position`, from 0, in the product's order.
    pub fn at(&self, position: usize) -> Option<ProductImageId> {
        self.0.get(position).map(|(id, _)| *id)
    }

    pub fn has(&self, image: ProductImageId) -> bool {
        self.0.iter().any(|(id, _)| *id == image)
    }

    /// Whether a scene of the product's ad may be of `kind`: a product
    /// shot needs an image, and the product in use a photo, since a
    /// screen is only ever shown as it is.
    pub fn allows(&self, kind: SceneKind) -> bool {
        match kind {
            SceneKind::Drawn | SceneKind::EndCard => true,
            SceneKind::ProductShot => !self.0.is_empty(),
            SceneKind::ProductInUse => self.0.iter().any(|(_, kind)| kind.is_photo()),
        }
    }

    /// The kinds a scene of the product's ad may be.
    pub fn allowed(&self) -> Vec<SceneKind> {
        SceneKind::ALL
            .into_iter()
            .filter(|kind| self.allows(*kind))
            .collect()
    }
}

/// A scene's picture as Bardo composed it from a product image: a file
/// in the project folder. It never came from a model and never goes to
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedShot {
    /// The file's name in the project folder.
    pub file: String,
    /// The product image it shows.
    pub image: ProductImageId,
    /// The frame's shape it was composed for.
    pub aspect: AspectRatio,
    /// The look it was composed in (#285); `None` for a picture made
    /// before looks, which is composed again.
    pub look: Option<ShotLook>,
    /// Tones of the product's image, for the user to build a backdrop
    /// from.
    pub tones: Vec<ShotColor>,
}

/// A scene's kind cannot change that way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SceneKindError {
    #[error("the plan has no scene {0}")]
    NoSuchScene(usize),
    /// The product's pictures do not allow the kind.
    #[error("the product's images do not allow a scene of kind {}", .0.code())]
    NotAllowed(SceneKind),
    /// Only the last scene is the end card.
    #[error("only the last scene can be the end card")]
    NotLast,
    /// The image named is not one of the product's.
    #[error("the product has no such image")]
    NoSuchImage,
    /// Only a scene Bardo composes has a product shot's look.
    #[error("the scene is not a product shot")]
    NotComposed,
}

/// What changing a scene's kind leaves to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindChange {
    /// Nothing changed.
    Same,
    /// The scene is composed now: its picture is made at once, free.
    Compose,
    /// The scene is drawn now. `needs_image` when it has no drawn image
    /// to go back to, so a paid drawing is the user's to ask for.
    /// `dropped` is the composed picture it no longer shows.
    Drawn {
        needs_image: bool,
        dropped: Option<ComposedShot>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pictures(kinds: &[ProductImageKind]) -> ProductPictures {
        ProductPictures::new(
            kinds
                .iter()
                .map(|kind| (ProductImageId::new(), *kind))
                .collect(),
        )
    }

    #[test]
    fn kinds_keep_their_codes() {
        for kind in SceneKind::ALL {
            assert_eq!(SceneKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(
            SceneKind::from_code(" product_shot "),
            Some(SceneKind::ProductShot)
        );
        assert_eq!(SceneKind::from_code("hero"), None);
        assert_eq!(SceneKind::default(), SceneKind::Drawn);
    }

    #[test]
    fn a_product_allows_the_kinds_its_images_can_show() {
        let none = pictures(&[]);
        assert_eq!(none.allowed(), [SceneKind::Drawn, SceneKind::EndCard]);
        assert_eq!(none.first(), None);

        let screens = pictures(&[ProductImageKind::Screen, ProductImageKind::Screen]);
        assert_eq!(
            screens.allowed(),
            [SceneKind::Drawn, SceneKind::ProductShot, SceneKind::EndCard],
            "a screen is never drawn in use"
        );

        let mixed = pictures(&[ProductImageKind::Screen, ProductImageKind::Photo]);
        assert_eq!(mixed.allowed(), SceneKind::ALL);
        assert_eq!(mixed.first(), mixed.at(0));
        assert!(mixed.has(mixed.at(1).unwrap()));
        assert!(!mixed.has(ProductImageId::new()));
        assert_eq!(mixed.at(2), None);
    }
}
