Ubi is JavaScript Backend Framework. Its very experimental, I create this for learning the Rust language. Currently only able to serve get request method. More ability will be added

To try it, download the release file or build yourself then add to your path.

Create file `.js` for example `main.js` with the following content:

```
// handler example using arrow function
// when using arrow function handler, you have to declare it before the router that call the arrow function
// because javascript doesnt hoist arrow function
const json = () => {
        return JSON.stringify({
                nama: "fuji"
        })
}

get("/", "text/html", home())
get("/json", "application/json", json())

// handler example using normal function
// support hoisting, so you can place it anywhere
function home(){
        return "helloooo"
}

listen(8080)
```

Finally simply run it by using `ubi your_file.js` for example `ubi main.js`
