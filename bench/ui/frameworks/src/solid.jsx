import { createSignal, createSelector, batch, For } from "solid-js";
import { render } from "solid-js/web";
import { buildData } from "./common.js";

const make = (id, l) => { const [label, setLabel] = createSignal(l); return { id, label, setLabel }; };
const Button = (p) => <div class="col-sm-6 smallpad"><button id={p.id} class="btn btn-primary btn-block" type="button" onClick={p.fn}>{p.text}</button></div>;

function App() {
  const [data, setData] = createSignal([]), [selected, setSelected] = createSignal(null);
  const isSelected = createSelector(selected);
  const remove = (id) => setData((d) => { const i = d.findIndex((r) => r.id === id); return [...d.slice(0, i), ...d.slice(i + 1)]; });
  return <div class="container">
    <div class="jumbotron"><div class="row">
      <div class="col-md-6"><h1>Solid keyed</h1></div>
      <div class="col-md-6"><div class="row">
        <Button id="run" text="Create 1,000 rows" fn={() => setData(buildData(1000, make))} />
        <Button id="runlots" text="Create 10,000 rows" fn={() => setData(buildData(10000, make))} />
        <Button id="add" text="Append 1,000 rows" fn={() => setData((d) => [...d, ...buildData(1000, make)])} />
        <Button id="update" text="Update every 10th row" fn={() => batch(() => { const d = data(); for (let i = 0; i < d.length; i += 10) d[i].setLabel((l) => l + " !!!"); })} />
        <Button id="clear" text="Clear" fn={() => setData([])} />
        <Button id="swaprows" text="Swap Rows" fn={() => { const d = data().slice(); if (d.length > 998) { const t = d[1]; d[1] = d[998]; d[998] = t; setData(d); } }} />
      </div></div>
    </div></div>
    <table class="table table-hover table-striped test-data"><tbody>
      <For each={data()}>{(row) => {
        const id = row.id;
        return <tr class={isSelected(id) ? "danger" : ""}>
          <td class="col-md-1" textContent={id} />
          <td class="col-md-4"><a class="lbl" onClick={[setSelected, id]} textContent={row.label()} /></td>
          <td class="col-md-1"><a class="remove" onClick={[remove, id]}><span class="remove glyphicon glyphicon-remove" aria-hidden="true" /></a></td>
          <td class="col-md-6" />
        </tr>;
      }}</For>
    </tbody></table>
  </div>;
}

render(App, document.getElementById("main"));
