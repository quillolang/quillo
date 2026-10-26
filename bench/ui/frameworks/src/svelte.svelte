<script>
  import { buildData } from "./common.js";
  class Row {
    label = $state("");
    constructor(id, label) { this.id = id; this.label = label; }
  }
  const make = (id, l) => new Row(id, l);
  let data = $state.raw([]);
  let selected = $state(0);
  const run = () => { data = buildData(1000, make); };
  const runLots = () => { data = buildData(10000, make); };
  const add = () => { data = [...data, ...buildData(1000, make)]; };
  const update = () => { for (let i = 0; i < data.length; i += 10) data[i].label += " !!!"; };
  const clear = () => { data = []; };
  const swapRows = () => { if (data.length > 998) { const d = data.slice(); const t = d[1]; d[1] = d[998]; d[998] = t; data = d; } };
  const remove = (row) => { data = data.filter((r) => r !== row); };
</script>

<div class="container">
  <div class="jumbotron"><div class="row">
    <div class="col-md-6"><h1>Svelte keyed</h1></div>
    <div class="col-md-6"><div class="row">
      <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="run" onclick={run}>Create 1,000 rows</button></div>
      <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="runlots" onclick={runLots}>Create 10,000 rows</button></div>
      <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="add" onclick={add}>Append 1,000 rows</button></div>
      <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="update" onclick={update}>Update every 10th row</button></div>
      <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="clear" onclick={clear}>Clear</button></div>
      <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="swaprows" onclick={swapRows}>Swap Rows</button></div>
    </div></div>
  </div></div>
  <table class="table table-hover table-striped test-data"><tbody>
    {#each data as row (row.id)}
      <tr class={selected === row.id ? "danger" : ""}>
        <td class="col-md-1">{row.id}</td>
        <td class="col-md-4"><a class="lbl" onclick={() => (selected = row.id)}>{row.label}</a></td>
        <td class="col-md-1"><a class="remove" onclick={() => remove(row)}><span class="remove glyphicon glyphicon-remove" aria-hidden="true"></span></a></td>
        <td class="col-md-6"></td>
      </tr>
    {/each}
  </tbody></table>
</div>
