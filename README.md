Ubi is Hybrid Fullstack Web Development Framework. Note this project is still very experimental. Currently only able to serve get request method. More ability will be added

To try it, download the release file or build yourself then add to your path (currently only support linux).

How to start an Ubi project?

1. Init a project using ubi init <project_name>. For example :
```
ubi init my_project
```

2. Cd to the project folder. For example :
```
cd my_project
```

3. Edit the config.json file. This is the file to configure the project, including configuring database (currently only PostgreSQL is supported, more database will be added soon)

4. It's done, let's start coding.

To create backend route, create file server.ts or server.py (you can chooses whether you want to code in TypeScript or Python) in routes folder.
To create sub route, just create new folder again in the parent folder, for example routes/users/server.ts.
The backend API url starts with /api, for example /api/users

To create frontend route, just create file ui.ubi in folder routes too.


Example of TypeScript route :
```
type Data = {
    tes: string
}

function get(): string {
    let hasil: Data = ubi.query("select * from data")
    return ubi.json(hasil)
}
```

Example of Python route :
```
from typing import TypedDict

class Data(TypedDict):
    tes: str

def get() -> str:
    hasil: Data = ubi.query("select * from data")
    return ubi.json(hasil)

```

Finally build your project by simply typing `ubi build` in the root directory of the project, internet connection is needed when building the project
