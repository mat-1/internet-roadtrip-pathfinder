# Internet Roadtrip Pathfinder

A somewhat reliable pathfinder for neal.fun's [Internet Roadtrip](https://neal.fun/internet-roadtrip).

You can download the pre-built userscript from here: https://ir.matdoes.dev/pathfinder.user.js. The standalone debug page is at https://ir.matdoes.dev/meowing.

If you're interested in reading the backstory of how the pathfinder came to be, I wrote a blog post about it: http://matdoes.dev/internet-roadtrip-pathfinder

The commit history was reset when the repo was published, some code contributions before this were by [@netux](https://github.com/netux).

## Development

```sh
# optional, build the userscript
cd userscript && bun run build && cd ..

# install Rust nightly, if you don't already have it
rustup install nightly && rustup default nightly

cargo r -r
# website is now running at http://localhost:2397/meowing
```
