// A small server-rendered todo app with a JSON API next to it.
// wisp run examples/todo.wisp, then open http://localhost:8080

let todos = []
let next_id = 1

fn add_todo(text: str) -> map {
    let todo = { id: next_id, text: trim(text), done: false }
    todos = todos |> push(todo)
    next_id += 1
    todo
}

fn find_todo(id: int) = todos |> find(fn(t) = t.id == id)

fn update_todo(id: int, changes: map) {
    todos = todos |> map(fn(t) = if t.id == id then t + changes else t)
    find_todo(id)
}

add_todo("Write a language")
add_todo("Serve it over HTTP")

// ---- views ---------------------------------------------------------------

view Layout(title: str, children) =
    <html>
        <head>
            <meta charset="utf-8"/>
            <title>{title}</title>
            <link rel="stylesheet" href="/static/style.css"/>
        </head>
        <body>{children}</body>
    </html>

view TodoItem(todo: map) =
    <li.todo #todo-{todo.id} class={if todo.done then "done"}>
        <form method="post" action="/todos/{todo.id}/toggle">
            <button title="toggle">{if todo.done then "☑" else "☐"}</button>
        </form>
        <span.text>{todo.text}</span>
    </li>

route GET / {
    let left = todos |> filter(fn(t) = not t.done) |> len
    <Layout title="Wisp todos">
        <h1>"Todos"</h1>
        <p.muted>"{left} of {len(todos)} left"</p>
        <ul.todos>{for t in todos => <TodoItem todo={t}/>}</ul>
        <form.add method="post" action="/todos">
            <input name="text" placeholder="What needs doing?" required/>
            <button>"Add"</button>
        </form>
    </Layout>
}

route GET /static/*path = file("public/" + path)

// ---- HTML form posts: form fields are matched like any payload --------------

route POST /todos { form: { text: str } } {
    if trim(text) != "" { add_todo(text) }
    redirect("/")
}

route POST /todos/:id(int)/toggle {
    let todo = find_todo(id) ?? reply 404 "no such todo"
    update_todo(id, { done: not todo.done })
    redirect("/")
}

// ---- JSON API -----------------------------------------------------------

route GET /api/todos = todos

route POST /api/todos { json: { text: str } } = reply 201 add_todo(text)

route POST /api/todos = reply 400 { error: "expected JSON like \{\"text\": \"...\"\}" }

route PATCH /api/todos/:id(int) { json: { done: bool } } {
    find_todo(id) ?? reply 404 { error: "no todo {id}" }
    update_todo(id, { done })
}

route DELETE /api/todos/:id(int) {
    find_todo(id) ?? reply 404 { error: "no todo {id}" }
    todos = todos |> filter(fn(t) = t.id != id)
    reply 204
}
