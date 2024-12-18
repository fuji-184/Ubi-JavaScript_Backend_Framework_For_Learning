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

