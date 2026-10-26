import { memo, useReducer } from "preact/compat";
import { render } from "preact";
import { buildData } from "./common.js";

function reducer({ data, selected }, a) {
  switch (a.type) {
    case "RUN": return { data: buildData(1000), selected: 0 };
    case "RUN_LOTS": return { data: buildData(10000), selected: 0 };
    case "ADD": return { data: data.concat(buildData(1000)), selected };
    case "UPDATE": { const d = data.slice(); for (let i = 0; i < d.length; i += 10) d[i] = { id: d[i].id, label: d[i].label + " !!!" }; return { data: d, selected }; }
    case "CLEAR": return { data: [], selected: 0 };
    case "SWAP_ROWS": { if (data.length <= 998) return { data, selected }; const d = data.slice(); const t = d[1]; d[1] = d[998]; d[998] = t; return { data: d, selected }; }
    case "REMOVE": { const i = data.findIndex((r) => r.id === a.id); return { data: [...data.slice(0, i), ...data.slice(i + 1)], selected }; }
    case "SELECT": return { data, selected: a.id };
  }
}

const Row = memo(({ selected, item, dispatch }) => (
  <tr class={selected ? "danger" : ""}>
    <td class="col-md-1">{item.id}</td>
    <td class="col-md-4"><a class="lbl" onClick={() => dispatch({ type: "SELECT", id: item.id })}>{item.label}</a></td>
    <td class="col-md-1"><a class="remove" onClick={() => dispatch({ type: "REMOVE", id: item.id })}><span class="remove glyphicon glyphicon-remove" aria-hidden="true" /></a></td>
    <td class="col-md-6" />
  </tr>
), (p, n) => p.selected === n.selected && p.item === n.item);

const Button = ({ id, cb, title }) => <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id={id} onClick={cb}>{title}</button></div>;

const Jumbotron = memo(({ dispatch }) => (
  <div class="jumbotron"><div class="row">
    <div class="col-md-6"><h1>Preact keyed</h1></div>
    <div class="col-md-6"><div class="row">
      <Button id="run" title="Create 1,000 rows" cb={() => dispatch({ type: "RUN" })} />
      <Button id="runlots" title="Create 10,000 rows" cb={() => dispatch({ type: "RUN_LOTS" })} />
      <Button id="add" title="Append 1,000 rows" cb={() => dispatch({ type: "ADD" })} />
      <Button id="update" title="Update every 10th row" cb={() => dispatch({ type: "UPDATE" })} />
      <Button id="clear" title="Clear" cb={() => dispatch({ type: "CLEAR" })} />
      <Button id="swaprows" title="Swap Rows" cb={() => dispatch({ type: "SWAP_ROWS" })} />
    </div></div>
  </div></div>
), () => true);

function Main() {
  const [{ data, selected }, dispatch] = useReducer(reducer, { data: [], selected: 0 });
  return <div class="container">
    <Jumbotron dispatch={dispatch} />
    <table class="table table-hover table-striped test-data"><tbody>
      {data.map((item) => <Row key={item.id} item={item} selected={selected === item.id} dispatch={dispatch} />)}
    </tbody></table>
  </div>;
}

render(<Main />, document.getElementById("main"));
