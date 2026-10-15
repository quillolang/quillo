// wisp run examples/hello.wisp
// then: curl localhost:8080/add/2/40

route GET / = <h1>"Hello from Wisp"</h1>

// Path parameters are variables. `:a(int)` only matches digits and gives an int.
route GET /hello/:name = "Hello, {name}!"
route GET /add/:a(int)/:b(int) = { sum: a + b }

// Query parameters are a pattern: typed, with defaults.
route GET /search ? { q: str, limit: int = 10 } = {
    query: q
    limit
    results: range(limit) |> map(fn(i) = "{q} #{i + 1}")
}
