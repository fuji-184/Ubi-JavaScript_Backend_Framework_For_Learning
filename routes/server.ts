type Data = {
    tes: string
}

function get(): string {
    let hasil: Data = ubi.query("select * from tes")
    return JSON.stringify(hasil)
}
