# Import rules: Family history folders

> Rules for pictures sorted into family history folders: one top folder per
> person or couple, then folders for eras, days and events. Copy this file,
> change it, and save it as _import-rules.md at the top of your folders (or
> choose it in the preview). Each rule is a "##" heading with its settings
> below. Rules run in order: folders from the top down, then the file name
> (the first file rule that matches wins, unless it says "then: continue"
> or "then: remove"). Add "off" on its own line to switch a rule off.
>
> applies to: top folder, folder, folder at level N, or file name, with
>   conditions such as "when the folder is event", "when the parent is
>   event", "when inside era" and "when {who} is a known name".
> pattern: a regular expression; named groups like (?<year>\d{4}) can be
>   used in "sets" as {year}. Also {folder}, {folder memory} (the memory a
>   folder rule gave, or else the folder's name), {parent}, {top}, {file}
>   and {photo day} (the day the picture was taken).
>   Filters: {subject|leading names}, {names|family}, {text|date},
>   {text|clean}, {n|number}.
> kind: what a folder is (family, era, date, event, contributor, couple,
>   household, skip). File rules can test it.
> sets: story, family, memory, date, place, series, photo_by, title,
>   description, note, people, maybe people (names that join People once
>   they are on the Names list below or already in the Journey).

## 1. Top folder = Story and Family
applies to: top folder
pattern: ^(?<names>[A-Z](?:[^\W\d_]|['’])+(?:\s*(?:,|and|&)\s*[A-Z](?:[^\W\d_]|['’])+)*)$
kind: family
sets: story = {folder}; family = {names|family}; people = {names}
then: stop
note: The person or couple the history is about.

## 1b. Other top folder = Story
applies to: top folder
pattern: ^.+$
kind: family
sets: story = {folder}; note = The top folder "{folder}" doesn't look like a family's name.
then: stop

## 2. Technical folder: skip
applies to: folder
pattern: ^(?:vivid|highlights|.*\.zip)$
case: ignore
kind: skip
then: stop
note: Edited copies or a download wrapper; their files act as if they sat in the folder above.

## 3. Date folder = exact date
applies to: folder
pattern: ^(?<year>(?:18|19|20)\d\d)[ ._-](?<month>\d{2})[ ._-](?<day>\d{2})$
kind: date
sets: date = {year}-{month}-{day}
then: stop

## 4. Era folder = date range
applies to: folder
pattern: ^(?<from>(?:18|19|20)\d\d)\s*-\s*(?<to>(?:18|19|20)\d\d)(?:\s*\((?<label>[^)]*)\)|\s+(?<label2>.+))?$
kind: era
sets: date = {from} to {to}
then: stop
note: A span of life. Not a memory.

## 5. Contributor folder ("From …")
applies to: folder
pattern: ^From (?<who>.+)$
kind: contributor
sets: photo_by = {who}
then: stop

## 6. Event folder with who took it ("… - Name")
applies to: folder
pattern: ^(?<event>.+?) - (?<who>[A-Z][a-z]+)$
kind: event
sets: memory = {event}; date = {event|date}; photo_by = {who}
then: stop

## 6b. Contributor folder (a known first name inside an event)
applies to: folder, when the parent is event and {who} is a known name
pattern: ^(?<who>[A-Z][a-z]+)$
kind: contributor
sets: photo_by = {who}
then: stop

## 7. Year and event folder = Memory with a year
applies to: folder
pattern: ^(?:(?<y1>(?:18|19|20)\d\d)\s+.+|.+\s+(?<y2>(?:18|19|20)\d\d))$
kind: event
sets: memory = {folder}; date = {y1}{y2}
then: stop

## 8. Couple folder = People
applies to: folder
pattern: ^(?<a>[A-Z](?:[^\W\d_]|['’])+)\s+(?:and|&)\s+(?<b>[A-Z](?:[^\W\d_]|['’])+)$
kind: couple
sets: people = {a}; people = {b}
then: stop

## 8b. Household surname folder
applies to: folder
pattern: ^[A-Z][a-z]+'?s$
kind: household
then: stop

## 9. Any other folder = event = Memory
applies to: folder
pattern: ^.+$
kind: event
sets: memory = {folder}
then: stop
note: A deeper event folder wins over a higher one.

## 10. Edition marks (edited copies)
applies to: file name
pattern: _Vivid_(?:ful|shr)\d?$|\s*-\s*Copy(?:\s*\(\d+\))?$|^Copy of\s+|\[\d\]$|\s*Cropped$
case: ignore
sets: note = An edited copy.
then: remove

## 11. Dated file name (in the creator's own scheme)
applies to: file name
pattern: ^(?<date>\d{8}|\d{6}|\d{4}-\d{2}-\d{2})[a-z]?\s*-\s*(?<subject>.+?)(?:\s*-\s*(?<place>[^-]*?(?:,\s*[A-Z]{2}\b|\b(?:home|Home|Lane|Park|Street|Church|Temple|Ranch|Farm)\b)[^-]*?))?(?:\s*-\s*(?<n>[a-z0-9]{1,2}))?$
sets: date = {date}; memory = {subject}; place = {place}; series = {n}; maybe people = {subject|leading names}
then: stop
note: YYYYMMDD-Subject-Place, with an optional series letter or number at the end.

## 12. Ultrasound pictures
applies to: file name
pattern: ^US[ _]?IMAGES?[ _-]*(?<n>\d+)$
case: ignore
sets: memory = Ultrasound pictures; series = {n|number}
then: stop

## 13. Scan batch (the year is when it was scanned)
applies to: file name
pattern: ^(?<scanyear>20\d\d)_(?<n>\d{4})(?<edit>_[a-z])?$
sets: memory = {folder memory}; series = {n|number}
photo date: ignore
then: stop
note: The date comes from the folder, or stays blank.

## 14. Camera file name with its date and time
applies to: file name
pattern: ^(?:PXL_|VID_?|IMG_)?(?<date>(?:19|20)\d{6})_\d{6}
case: ignore
sets: date = {date}
then: continue

## 15. Camera file name under an event folder
applies to: file name, when the folder is event or contributor
pattern: ^(?:IMG_E?\d+|PXL_\d{8}_\d+.*|(?:VID_?)?\d{8}_\d{6}.*|DSCN?_?\d+|DSCF\d+|P\d{7}|image\d+|EECM\d+|SCAN-?\d+|MVI_\d+|VID_\d+)$
case: ignore
sets: memory = {folder memory}
then: stop

## 15b. Camera file name under a date folder
applies to: file name, when the folder is date
pattern: ^(?:IMG_E?\d+|PXL_\d{8}_\d+.*|(?:VID_?)?\d{8}_\d{6}.*|DSCN?_?\d+|DSCF\d+|P\d{7}|image\d+|EECM\d+|SCAN-?\d+|MVI_\d+|VID_\d+)$
case: ignore
sets: memory = {folder}
then: stop

## 15c. Camera file name anywhere else: one memory per day
applies to: file name
pattern: ^(?:IMG_E?\d+|PXL_\d{8}_\d+.*|(?:VID_?)?\d{8}_\d{6}.*|DSCN?_?\d+|DSCF\d+|P\d{7}|image\d+|EECM\d+|SCAN-?\d+|MVI_\d+|VID_\d+)$
case: ignore
sets: memory = {photo day}
then: stop

## 16. Scan or album page
applies to: file name
pattern: ^(?<album>.+?)\s+(?:P(?<page>\d+)(?<photo>[a-z]\d?)\b|Page\s*(?<page2>\d+)|Flat-\s*(?<n>\d+)|Scans?\s*(?<n2>\d+))
case: ignore
sets: memory = {folder memory}; series = {page|number}{photo}{page2|number}{n|number}{n2|number}; date = {file|date}
then: stop

## 17. Series under an event folder
applies to: file name, when the folder is event or contributor
pattern: ^(?<stem>.+?)(?:_(?<n>\d{2,})|\s*\((?<n2>\d+)\)|\s*\((?<n3>\d+)\s*of\s*\d+\)|\s*-\s*(?<n4>\d+))$
sets: memory = {folder memory}; series = {n|number}{n2|number}{n3|number}{n4|number}; date = {stem|date}; maybe people = {stem|leading names}
then: stop
note: The stem is only a series name; the folder names the memory.

## 17b. Series anywhere else
applies to: file name
pattern: ^(?<stem>.+?)(?:_(?<n>\d{2,})|\s*\((?<n2>\d+)\)|\s*\((?<n3>\d+)\s*of\s*\d+\)|\s*-\s*(?<n4>\d+))$
sets: memory = {stem|clean}; series = {n|number}{n2|number}{n3|number}{n4|number}; date = {stem|date}; maybe people = {stem|leading names}
then: stop

## 18. Description
applies to: file name
pattern: ^(?<text>.*[^\W\d_].*)$
sets: memory = {text|clean}; date = {text|date}; maybe people = {text|leading names}
then: stop
note: Anything else: the name without its date and series number.

# Names

> First names (or full names) of people, one per line. Names found in file
> names join People only when they are here or already in the Journey; the
> rest are listed to confirm after each preview.

# Initials

> Initials used in file names and who they stand for, such as:
> WJR = Walter J. Rowan

# Spelling fixes

> Misspellings fixed in every folder and file name before the rules run:
> Vaccation = Vacation

# Skip folders

> Folder names whose files act as if they sat in the folder above.

# Settings

day 01 means month only: no
later photo years: 10
