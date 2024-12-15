let tes = "tes"

get("/", tes);
get("/about", "Ini adalah halaman About");
get("/contact", "Hubungi kami di contact@example.com");
listen(3000);
