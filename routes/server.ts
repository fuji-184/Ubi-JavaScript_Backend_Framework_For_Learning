type Data = { tes: string; };

function get(): string {
    let _ = ubi.query("create table if not exists tes (nama varchar(100))");
    let _ = ubi.query("insert into tes(nama) values('helloooo')");
    let hasil: Data = ubi.query("select * from tes");

    return ubi.json(hasil);
}
