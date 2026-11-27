---
id: world
title: World: characters, places and objects
group: production
place: world
tour: world
---

# World: characters, places and objects

The **World** holds what comes back across your videos: characters (a grandmother who narrates the family stories, a detective, a mascot), places (her kitchen, the detective's office) and objects (a motorbike, a guitar, a pocket watch). Each has a fixed appearance, notes that always or never hold, and a **reference picture** per look, so every video that picks it shows the same face, room or thing. The project's world lives on [World](bardo:go/world); what every project can pick lives on the [global library's World tab](bardo:go/library/world). Writing and editing costs nothing; drawing a picture is paid, and shows its cost first. [Show me the screen](bardo:tour/world).

<a id="kinds"></a>
## Characters, places and objects

The screen has one tab each for **Characters**, **Places** and **Objects**. They share the same form (name, appearance, notes) and the same pictures, with these differences:

- A **character** is always an original person, never a real one. Its pictures are portraits, and it lists its [belongings](#belongings).
- A **place** is where scenes happen. Its appearance is called **Description** (layout, light, materials) and its pictures are wide, in the shape of a landscape frame. A scene set in a picked place is drawn with its picture.
- An **object** is something scenes show. It may [belong to a character](#belongings). A scene that shows a picked object is drawn with its picture when it has one for the video's look, from its description otherwise.

A name is unique per kind: a place and an object may both be called *Rosa's*.

<a id="characters"></a>
## Your characters

The list holds the items of the open tab, of the open project (or of the global library, on its tab). **New character**, **New place** or **New object** opens an empty form; picking an item opens it for editing. Archived items are hidden until you tick **Show archived**. An item's name is unique among the items of the same kind and the same owner, archived ones included, ignoring case: a project and the global library may each have a Rosa.

<a id="appearance"></a>
## Appearance and notes

- **Name** (required): at most 60 characters. The scene plan calls the character by this name.
- **Appearance** (required): face, hair, build and clothes, at most 1,500 characters. Every reference picture is drawn from this text, and the scene plan copies it as it is into the cast.
- **Always** and **Never**: what always holds (carries a red umbrella) and what never happens (never speaks on screen), at most 1,000 characters each. The scene planner reads them in every video that picks the character.

The same fields hold for places and objects. A new **appearance** drops the item's reference pictures in every look and the portraits of the videos that show it, since they no longer match. Before saving, Bardo says which looks and how many videos: **Save and drop them** confirms. Each picture is drawn again, and paid again, the next time it is needed. A new name or new notes keep the pictures.

<a id="belongings"></a>
## Belongings

An object can belong to a character: on the object, **Belongs to** names the character, or **Nobody**. It is saved as soon as you choose. A project's object may belong to a character of the project or of the global library; a global object only to a global character. The character's form lists its **Belongings**, and the scene planner reads them with the character.

On [Who appears](scenes.md#who-appears), picking a character offers its belongings first under **Belongings of the picked characters**. A belonging is never picked on its own: you add the ones this video shows.

<a id="pictures"></a>
## Reference pictures

A reference picture is the character's portrait, or the place's or object's picture, in one **look**: a style of the catalog (photo, cartoon, manga and so on), or **a channel's notes**, kept by the channel and the text of its aesthetic notes. Each picture has its own record of how it was made (provider, model, prompt, when, and whether a video drew it, you drew it here, you saved it from a video's cast or you imported it), and lives in the project's `World` folder, or in `Global\World` under the base folder for the global library.

- **Before the first scene that needs it**: a video that shows the character in a look with no picture draws one before that scene, as it does for any [person who comes back](scenes.md#cast), paid under that video. The picture is then kept in the world.
- **On demand**: pick a look, see the estimate and press **Draw picture**. The new picture waits for review: **Keep** makes it the one videos in that look use, replacing the earlier one, and **Discard** throws it away. It is paid under the character's project; a global character's under no project.

<a id="import"></a>
## Importing a picture

A place's or an object's picture can also be **imported**: pick the look, press **Import picture…**, confirm that you may use the picture in your videos and that it shows no real person, then **Choose picture…** (PNG, JPEG or WebP, within the limits of a [product image](products.md#pictures)). It becomes the kept picture of that look at once, replacing the one there, and keeps your confirmation with its time. Importing costs nothing. A character's portrait cannot be imported.

<a id="reuse"></a>
## Reusing a picture

Every later video in the same look gets the kept picture for free: the estimate does not count it and no paid call is made. Other notes on a channel are another look, so they draw a new picture. Deleting the video that first drew a picture leaves the picture. **Remove** drops a kept picture; the next video in that look draws a new one.

<a id="archive"></a>
## Duplicate, archive and delete

- **Duplicate** copies the fields and the kept pictures under a new name.
- **Archive** hides the item from [Who appears](scenes.md#who-appears); videos that already picked it keep it. **Restore** brings it back.
- **Delete** removes the item with its pictures. It is refused while a video in progress uses it, and the message names those videos: archive the item, or archive or delete those videos first. Videos that showed it keep their pictures; their cast member or place becomes one of the video's own, and its objects stop belonging to a deleted character.

<a id="sharing"></a>
## Sharing with other projects

Under the actions, **Sharing** marks an item **Shareable** for your other projects, exports a copy to one of them, makes it global or saves it to a file; **Import from another project** and **Open item file…** at the top bring one in. A linked copy follows every change of its original and its pictures arrive just after. See [Sharing between projects](global-library.md#sharing).

<a id="save"></a>
## Saving a video's person to the world

A person of a video's cast can become a character: on the [Scenes](scenes.md#cast) stage, **Save to World** keeps them in the project's world and **Save to global library** in the global library, with their portrait for the video's look. The video's person then is that character.

<a id="save-place"></a>
## Saving a video's place to the world

A place of a video's plan can become a place of the world the same way: under the place of a scene, **Save to World** or **Save to global library**. The plan's place then is that place (it reads **From the world**) and is picked for the video. A plan's place has no picture of its own: its first picture is drawn when a video needs it.

<a id="backup"></a>
## Backups and the settings file

[Backups](backup.md) carry the characters, places, objects and their pictures, imported ones included. The [settings file](settings.md) does not: it carries preferences, not your work. Deleting a project deletes its world.
