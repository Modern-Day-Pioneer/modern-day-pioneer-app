# Modern Day Pioneer

The code for **Modern Day Pioneer** (working title), a Windows app for gathering a family's photos, recordings and stories and exporting them as a family history that opens in any web browser, with no internet and no special software. Website: [moderndaypioneer.org](https://moderndaypioneer.org). Windows installers are on this repository's [Releases](https://github.com/mbwamkali/modern-day-pioneer-app/releases) page.

- `mdp-core/`: the Journey, catalog, import, export, backup and background jobs, as a Rust library with no screens. Run its checks with `cargo test` inside `mdp-core`.
- `mdp-app/`: the Tauri 2 app (plain HTML and JavaScript screens in `mdp-app/ui`). See [mdp-app/README.md](mdp-app/README.md).

## Project proposal

### Summary*Modern Day Pioneer* is the working title of this project. It aims to help families preserve and tell their stories in a form the next generation can still open and use. A family gathers its photos, recordings, videos and stories in one application, connects them to the people and places they belong to, and produces a family history that anyone can open in a web browser, with no internet connection, no special software and no account. The aim is for it to stay usable for decades; no technology can guarantee that, so the design focuses on reducing the risks rather than claiming to remove them.

### What we are doing

Families have their history scattered across phones, PCs and cloud services like Google Photos. It's mostly photos, voice recordings, videos and stories that live in people's heads. The links that give it meaning, such as who is in a photo, where it was taken and the story behind it, are rarely recorded anywhere. Even when someone does put it together, it usually sits in an app, service or format that can disappear or become unreadable, the way home videos on BetaMax did. Many families already know this from boxes of slides in the attic: the projector is long gone, and the pictures survive only because you can still hold a slide up to the light.

This application lets a family gather that media from their own devices and from online services they already use, connect it to people, places and stories, and add descriptions by typing or by voice. Then it exports the result in a form chosen to give it the best chance of lasting. That's one HTML5 page anyone can open in a browser, with every original photo, recording and text kept beside it in plain, widely used formats. The exported result works fully offline with no extra software, so it doesn't depend on a company or service staying around.

### Why it matters

- **Memories are being lost to scattered storage.** Photos sit on many devices and accounts, and the context that makes them meaningful is held only in people's memories.
- **Digital formats and services expire.** Formats lose support and services close. A family history is only worth building if it can still be opened decades from now.
- **Voices and stories are the hardest part to keep.** A recording of a grandparent telling a story is irreplaceable, and today it rarely gets saved with the photos it describes.

### Objectives

1. **Gather:** bring a family's photos, recordings, videos and writing into one place, from their own devices and selected online services.
2. **Connect:** link every item to the people, places and stories it belongs to.
3. **Tell:** let families add descriptions and stories by typing or by speaking.
4. **Preserve:** produce a result designed to stay viewable for at least 5 to 10 years and, ideally, for decades, independent of the app.

### How the application solves it

| The problem | What the application does |
|---|---|
| Photos, recordings and documents are scattered across phones, computers and online accounts. | **Gathers everything in one place.** Families bring in what is already on their own devices and from online services they use, without altering the originals. |
| The context that gives media meaning (who, where, why) lives only in people's memories. | **Connects every item** to the people, places and stories it belongs to, and saves those connections in an open, readable format. |
| Stories and voices are rarely captured, and are irreplaceable once lost. | **Records voices and stories** by typing or speaking, keeping every recording with a written transcript. |
| Collecting and organizing a family history takes real tools, not just a viewer. | **Provides a working app** for Windows and Android (Mac later), installed from the app stores, that uses each device's camera, microphone and files. |
| Apps and online services close, and anything that depends on them goes with them. | **Produces a history designed to stand on its own:** it opens with no internet and no special software, and all content is kept on the family's own device. |
| Formats and players become obsolete, like Betamax and the slide projector. | **Exports in the most widely adopted formats:** one HTML5 page to open, with every original photo, story and recording kept beside it as ordinary files, so there is a second way in. |

### Who it is for

- **The family historian:** the person who gathers the photos, records relatives' voices and writes down the stories. They use the application.
- **The storytellers:** parents, grandparents and relatives whose memories and voices are recorded, often by sitting with the family historian for an afternoon.
- **The family who receives it:** children, grandchildren and relatives today and in the future. They never need the application; they simply open the finished history.

### What a family ends up with

Picture a grandchild in 2045 who finds a folder copied from an old drive. They open one file, and their great-grandmother's wedding photo appears with her name, the church where it was taken, and a short recording of her telling how she met her husband, with the words written out underneath. A click on her name shows every other photo and story she appears in. If that page ever fails to open, and as long as the folder itself has survived, the photo is still an ordinary picture file in the folder, the story is still a text file, and the recording is still a sound file anyone can play.

### What this project is not

The project has one focus: telling a family's story. Some things are deliberately left to other tools, so that the work stays focused enough to deliver a useful first solution. Bringing content in from those tools is still a goal, wherever that is practical.

- **It is not a family history research tool.** Many established tools already help people search records and trace their ancestors. This project starts from what a family already has and knows.
- **It is not a family tree or genealogy viewer.** A family tree can be part of the story, but it is only a small part of a person's life. The focus is on the photos, voices and stories that show who people were and how they lived.
- **It is not about public figures or fictional characters.** It is designed for telling the stories of groups of people, above all families, whether by birth or by choice. Individual family members can be highlighted, but it is not built for reporting on public figures or invented characters.

### Scope

- **It is not an online service.** The family's content is kept on their own devices, and the finished history works without the internet.
- **The application is not needed to view the history.** It is only the tool for creating it; the result opens on its own.
- **It is not tied to one company's format.** Everything it produces uses common, openly published formats.
- **First release: Windows computers and Android phones.** Mac follows later.

### Risks and limits

All technology has risks and all processes fall short. The purpose of this project is to reduce those risks and add value, not to eliminate them or offer a perfect solution. Even with the choices above:

- **Formats can still lose support.** We choose the most widely adopted formats to lower this risk, but no one can know which formats will be readable in 30 years.
- **Storage fails.** Hard drives, USB sticks, phones and memory cards wear out or get lost. A history survives only if families keep more than one copy and refresh them over time.
- **The application itself will change or end.** That is why the finished history does not depend on it, but future updates to the app are not assured.
- **Content is only as good as what is captured.** Automatic transcripts contain mistakes, and details people don't record cannot be recovered later.

### In closing

Betamax tapes and boxes of slides teach the same lesson: the memories were never the problem; the technology needed to reach them was. This project is built around that lesson. The application does the work of gathering and connecting a family's history today, and what it produces is designed to outlast the application itself, improving the chances that the stories, faces and voices that matter most can still be opened by the generations who will want them most.
