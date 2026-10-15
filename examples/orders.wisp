// Payload-based routing: the same path, different handlers, chosen by
// what is inside the JSON body. Routes are tried top to bottom.
//
//   curl -d '{"type":"order","customer":{"id":7},"total":120}' localhost:8080/orders
//   curl -d '{"type":"quote","items":[{"price":5},{"price":7}]}' localhost:8080/orders

let orders = []

fn price(items: list) -> num = items |> map(fn(i) = i.price ?? 0) |> sum

route POST /orders { json: { type: "order" | "refund", customer.id: int as id, total: num } } {
    let order = { id: len(orders) + 1, type: request.json.type, customer: id, total }
    orders = orders |> push(order)
    reply 201 order
}

route POST /orders { json: { type: "quote", items: list } } = { quote: price(items) }

route POST /orders = reply 422 { error: "expected type order, refund or quote" }

route GET /orders ? { customer: int = nil } =
    if customer == nil then orders
    else orders |> filter(fn(o) = o.customer == customer)

// Middleware is just a function in a pipeline. `reply` stops the request.
let admin = {
    only: fn(req) =
        if req.headers["x-api-key"] == env("ADMIN_KEY", "dev-key") then req
        else reply 401 { error: "missing or wrong x-api-key" }
}

route DELETE /orders = request |> admin.only |> fn(_) {
    let n = len(orders)
    orders = []
    { deleted: n }
}
