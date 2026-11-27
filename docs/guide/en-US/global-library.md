---
id: global-library
title: Global library
group: getting-started
place: library
tour: library
---

# Global library

The **Global library** holds what every project can use: characters, places, objects, narrators, products and templates. [Show me](bardo:tour/library).

<a id="tabs"></a>
## Its tabs

The library has four tabs:

- **World**: characters, places and objects any project's videos can pick ([World](world.md)).
- **Narrators**: the voices and faces that speak your videos ([Narrators](personas.md)).
- **Products**: the products your ads show, with their briefs and pictures ([Products](products.md)).
- **Templates**: the instructions Claude writes from ([Templates](templates.md)).

<a id="libraries"></a>
## What each holds

Each tab is the same screen you know, with the same lists and forms: create, edit and archive items there as before.

<a id="projects"></a>
## Global and the project's own

Characters, places and objects can be the project's own, on its [World](world.md), or global, on this tab: a video picks from both. Narrators and products work the same way: a project's **Narrators** and **Products** entries list its own and the global ones it uses, tagged **Global**, and these tabs list the global ones. Editing a global item changes it everywhere it is used, so Bardo lists those places first; **Bring into the project** makes a copy of the project's own instead ([Your narrators](personas.md#library), [Your products](products.md#library)). Each kind of template has a global template here, and a project may have its own ([This project's or global](templates.md#project)). Deleting a project deletes its own narrators, products and templates; the global ones stay.

<a id="sharing"></a>
## Sharing between projects

A project's characters, places, objects, narrators, products and templates are its own, but your other projects can use them too. Under an item's form, **Sharing** shows how:

- **Shareable** lets your other projects import it. Untick it and every linked copy of it becomes independent.
- **Export to another project** puts an independent copy, with its pictures, into the project you pick.
- **Make global** moves it here, to the global library. The project keeps using it, and every linked copy of it in other projects becomes the global item itself: what pointed at the copy now points at the global one. For a template, its text becomes the next version of the global template, after Bardo lists the projects that use the global one, and the project goes back to the global template.
- **Save to file…** writes a `.bardo-item` file, to open on another computer.

At the top of a project's **World**, **Narrators**, **Products** and **Templates**, **Import from another project** lists what your other projects marked **Shareable**, archived projects included. Pick **Linked copy** or **Independent copy**:

- A **linked copy** follows every change of the original, in the same save: it is read-only, tagged **Linked**, and says what it follows. A linked template takes each new version of the original. A world item's pictures arrive just after; while they are being copied it reads **Updating**, and **Waiting for its folder** while a folder is on a drive that is not connected. **Unlink** makes it the project's own, with all it holds. When the original is deleted, stops being shared or its project is deleted, the copy becomes independent with all it held, and says so once.
- An **independent copy** is the project's own from the start. A template copies only its latest version.

When the project already has one of the name, Bardo suggests the name followed by the other project's. **Open item file…** opens a `.bardo-item` (or a narrator's `.bardo-persona`) as an independent copy. A file never holds a photo of a real person: a narrator's photo look is left out. A product's file holds its pictures, so opening one asks you to confirm you may use them and the brand. A file made by a newer Bardo is refused: update Bardo to open it.
