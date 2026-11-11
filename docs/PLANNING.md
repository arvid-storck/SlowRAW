### Bugs

- Does not read the ".rrdata" generated from lightroom properly
- Cant open nikon HE* files (an unstable fix is to apply the patch from "https://github.com/dnglab/dnglab/pull/835")
- Make filterbox uniform in width, allow names to span multiple rows

### Things to change

Easy

- Fix compiling documantation
- Add all dev tools on CODEMAP.md
- Remove Clear ".rrdata" files button
- Fix "src\i18n\locales\en.json", keys: "filteredTotal", "total", "total_one", "total_other". Add key: "filteredTotalOne".

  Should read:
  
  "x images" if more or equal to 2 and/or equal to 0.
  
  "x image" if its 1
- Sorting defult should be date captured, newest first

Not easy but not risky
  
- Make a build cashe button
- Remove the 500 images per page thing and dynamilcy load the next page when scrolling down and unload the last page, maby keep 3st 166 images per page files open (previous, current, next)?

Hard and/or risky

- Rename variables, fuctions and classes to to camel case, and upper camel case. But only to make it uniform, not when a langage has a recomended style already.
- Fix general ineffiencies left



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
- How is an image without a date sorted?

