# Getting started

Run the site from the checkout:

```sh
quillo run app/mdsite --watch
```

Then open <http://localhost:8080>.

## Adding a page

1. Write `pages/recipes.md`, starting with a `# ` heading: that is the page's title.
2. Add it to `NAV` in `main.qwe`:

   ```quillo
   { path: "/recipes", file: "recipes.md", group: "" },
   ```

3. Save. The sidebar shows it, and `/recipes` serves it.

A page not in `NAV` is not served: the list is what the site has.
