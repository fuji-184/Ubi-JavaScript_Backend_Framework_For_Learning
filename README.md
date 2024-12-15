Ubi is JavaScript Backend Framework. Its very experimental, I create this for learning the Rust language. Currently only able to serve plain text using get request method. More ability will be added

To try it, download the release file or build yourself then add to your path.

Create file `.js` for example `main.js` with the following content:

```
let tes = "Hello!"

get("/", tes)
get("/hello", "Hello again!")

listen(3000)
```

Finally simply run it by using `ubi your_file.js` for example `ubi main.js`
