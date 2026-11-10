### Bugs

- Does not read the ".rrdata" generated from lightroom properly
- Cant open nikon HE* files
- Make filterbox uniform in width, allow names to span multiple rows

### Things to change

- Remove Clear ".rrdata" files button
- Make a build cashe button
- Remove "xx matching yy total" text, just list the number of images currently filtred as it did before
- Add all dev tools on CODEMAP.md
- Fix "src\i18n\locales\en.json", keys: "filteredTotal", "total", "total_one", "total_other". Add key: "filteredTotalOne", should read "x images" or "x image" if its one
- Rename variables, fuctions and classes to to camel case, and upper camel case. But only to make it uniform, not when a langage requite a diffrent style.
- Fix general ineffiencies left
- Fix compiling documantation
- Remove the 500 images per page thing and dynamilcy load the next page when scrolling down and unload the last page, maby keep 3st 200images per page files open (previous, current, next)?

### Things to consider (Do not implelent unless you need to or want to do and its benfits the program)

- Remove unnecessary validation tests
- Higher quality x-trans decoder, consider "Markesteijn 1-pass"?

### Need to be investigate

- Test SlowRAW on linux
- Test SlowRAW on mac
- Test more file formats for compatability
- If built in lens correction data exist that is not used currently

### Questions about the code

- How does the queue part work?
- Is there still android code left?
- How is edits stored?

