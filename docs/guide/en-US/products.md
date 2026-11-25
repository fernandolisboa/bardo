---
id: products
title: Products
group: strategy
place: products
tour: products
---

# Products

A product is something you advertise: a digital product such as an app or a course, a physical product, a service, or an idea that does not exist yet. You describe it once and reuse it in every ad, on any channel. Products live on [Products](bardo:go/products). The brief, the images and the logo cost nothing; only [filling the sheet](#research) with Claude is paid, and it shows its cost first. [Show me the screen](bardo:tour/products), or [part 2 of the tour](bardo:tour/products-sheet) for the sheet and its claims.

<a id="library"></a>
## Your products

The list holds every product with its kind. **New product** opens an empty brief; picking a product opens it for editing. Archived products are hidden until you tick **Show archived**.

<a id="brief"></a>
## The brief

The brief is what you tell Bardo about the product, and the only source an ad has for it:

- **Name**: unique among your products, archived ones included, at most 80 characters.
- **Kind**: digital product, physical product, service or idea. An ad for an idea presents it as an idea, never as something anyone can buy today.
- **What it is** (required): what it does and how, in a few sentences.
- **Who it is for**: who buys or uses it, and what they want.
- **Offer**, **Link** and **Call to action**: the price or discount, the address the ad points to, and what the viewer should do. A link without `https://` gets it; only web addresses are accepted.

<a id="facts"></a>
## Facts you stand behind

**Facts you stand behind** takes one statement per line, in your own words: features, numbers, prices, delivery times. Bullets and dashes you paste at the start of a line are removed and blank lines are dropped; Bardo shows how many facts it read. An ad may state only these facts until you confirm others, so leave out anything you cannot back. There can be up to 30 facts of at most 300 characters each.

<a id="owner"></a>
## Whose product it is

**Whose it is** says whether the product is yours or a client's. It sets how the posts of its ads are disclosed: as your own brand, or as a paid partnership when the product is a client's. **An ad must say** holds what every ad says, such as a required legal line; **An ad must never say** holds the words, promises or comparisons to keep out.

<a id="pictures"></a>
## Images

A saved product can hold up to three images: PNG, JPEG or WebP, 600 to 6,000 px a side, a long side at most three times the short one, and at most 10 MB. Before choosing a file, tick **I may use this picture and the brand it shows in ads**; Bardo keeps that confirmation, with its time, beside the picture. Your file is kept exactly as it is.

Each image is a **Photo** (the product, its packaging, a place) or a **Screen** (a screenshot of an app or site). Bardo only ever shows a screen as it is and never redraws it. A photo saved with transparency (a PNG or WebP with an alpha channel) is taken as already cut out and marked **Cut out**, ready to place on other backgrounds. Bardo goes by what the file says, so save a picture that way only when its background really is transparent. **Remove**, under an image, asks before it drops the picture and its confirmation. A product can also have no images: its ads then use drawn scenes and an end card of text.

<a id="logo"></a>
## The logo

The optional logo is a PNG, at least 64 px on each side and 200 px on its longer side, used only on the ad's end card. A transparent background looks best. It asks for the same confirmation as the images. **Replace logo…** swaps it and **Remove logo** drops it after you confirm.

Images and the logo are kept in Bardo's database, so every backup, including one of the database only, carries them ([Backup and restore](backup.md)). A settings file carries the briefs but not the pictures; a product it replaces keeps the pictures it has ([Settings file](backup.md#settings-file)).

<a id="sheet"></a>
## The sheet

**Brief** and **Sheet**, at the top of a saved product, switch between what you wrote and what Bardo knows beyond it. The sheet holds:

- **Claims**: what an ad may say about the product, each with where it came from.
- **About the product**: a summary, its category and how it looks.
- **Who it is for**: up to 8 groups of buyers, with what each wants and what holds it back.
- **Objections and answers** and **Alternatives**: up to 12 of each.
- **Pages read**: the pages the last research read, each with a link.

Everything but the list of pages is yours to edit; **Save the sheet** keeps the changes. **Restricted category** marks a product in a category networks restrict: health and medicines, financial products, alcohol, gambling, weight loss, adult or political. Bardo then reminds you to check each network's rules before you publish, and blocks nothing.

Backups carry the sheet with the rest of the database. A settings file does not; a product it replaces keeps the sheet it has.

<a id="research"></a>
## Filling the sheet

**Fill the sheet** gives three ways, and you can use more than one:

1. **Research on the web**: Claude reads the product's page first, starting from the brief's **Link**, then searches what is public about the product and its market. One research makes at most 8 searches and reads at most 5 web pages, and it is told to leave PDFs and other files unread. A second call turns its notes into the sheet. Without a link the research searches by the product's name.
2. **Write from my brief**: one call and no web. Claude organizes what your brief and images already say.
3. **Fill in by hand**: no call and no cost. The sheet starts with the facts of your brief.

The first two need your Claude key ([API keys](api-keys.md)) and are paid. Before a research starts, Bardo shows the most it can cost, with every search and web page at its limit; once researches have run, it also shows what one usually costs. A research is billed by its tokens and by its searches, and both appear on [Costs](costs.md#rates). A run that would take Claude past its budget asks first.

While a run goes, the sheet cannot be edited and the product cannot be deleted, and for a moment after you stop one, while the request it sent is given up; a brief you save meanwhile is the one the sheet is written from. The run also appears on [Jobs](jobs.md), where you can stop it; retrying a run that failed after its research keeps the research and only writes the sheet again. A research that failed after Claude billed part of it is in Costs for that part, and Bardo does not run it again by itself. **Retry** on Jobs checks what the run still has to pay for like a new run: when a new research at the most it can cost, or the sheet alone, would reach Claude's budget, Bardo asks first.

A run adds to the sheet: it never changes what you wrote or reviewed, so **Research again** is safe after you change the brief. A restricted category you chose stays, **None** included. A group, an objection, an alternative or a page you removed is not remembered, so a later run can add it again. A sheet holds at most 80 claims, not counting the rejected ones; a run that finds it full says how many claims it left out.

<a id="claims"></a>
## Claims

An ad states only **confirmed** claims. A claim is one statement of at most 300 characters and a kind: benefit, feature, proof, offer or comparison.

- The facts of your brief and the claims you add under **Add a claim** start confirmed. A fact you add to the brief later joins the sheet when you save the brief; when a model's claim already says the same, that claim becomes yours, confirmed.
- Whatever a model wrote waits under **Waiting for you**, with its source: Claude's reading of your brief, the product's page (the address of your link, or one under it) or another web page. **Open the page** shows where it came from; a claim read from your brief has no page behind it, so check it against what you know. **Confirm** it only when it is true and you can back it; **Reject** the rest. **Edit** changes the words or the kind before you decide.
- A rejected claim stays in the list, so the next research does not bring it back as new. **Bring back** returns a claim a model wrote to the waiting ones, and one of your own to the confirmed ones.

**Sensitive** marks a claim that needs care: a number, a superlative, a promise about health, money or results, a guarantee, a deadline or a named competitor. Models mark them as they write, and you can tick or clear the mark; a number you write into a claim you had cleared marks it again. A sensitive claim can be confirmed and used; each one a video uses is listed again before the video is published.

<a id="archive"></a>
## Duplicating, archiving and deleting

**Duplicate** saves the changes you have made, then copies the brief, the images, the logo and the sheet under a new name, for a variant of the same product. **Archive** hides a product you no longer advertise from the list and from new ads; **Restore** brings it back. **Delete…** removes the product with its images, logo and sheet after you confirm, and is refused while any video uses the product (archive it instead) or while a run fills its sheet.
