import { createApp, shallowRef, ref, triggerRef } from "vue";
import { buildData } from "./common.js";

createApp({
  setup() {
    const rows = shallowRef([]), selected = ref(0);
    return {
      rows, selected,
      run() { rows.value = buildData(1000); selected.value = 0; },
      runLots() { rows.value = buildData(10000); selected.value = 0; },
      add() { rows.value = rows.value.concat(buildData(1000)); },
      update() { const r = rows.value; for (let i = 0; i < r.length; i += 10) r[i].label += " !!!"; triggerRef(rows); },
      clear() { rows.value = []; selected.value = 0; },
      swapRows() { const r = rows.value; if (r.length > 998) { const t = r[1]; r[1] = r[998]; r[998] = t; triggerRef(rows); } },
      remove(id) { const r = rows.value; r.splice(r.findIndex((x) => x.id === id), 1); triggerRef(rows); },
      select(id) { selected.value = id; },
    };
  },
  template: `<div class="container">
    <div class="jumbotron"><div class="row">
      <div class="col-md-6"><h1>Vue keyed</h1></div>
      <div class="col-md-6"><div class="row">
        <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="run" @click="run">Create 1,000 rows</button></div>
        <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="runlots" @click="runLots">Create 10,000 rows</button></div>
        <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="add" @click="add">Append 1,000 rows</button></div>
        <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="update" @click="update">Update every 10th row</button></div>
        <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="clear" @click="clear">Clear</button></div>
        <div class="col-sm-6 smallpad"><button type="button" class="btn btn-primary btn-block" id="swaprows" @click="swapRows">Swap Rows</button></div>
      </div></div>
    </div></div>
    <table class="table table-hover table-striped test-data"><tbody>
      <tr v-for="row of rows" :key="row.id" :class="{ danger: row.id === selected }">
        <td class="col-md-1">{{ row.id }}</td>
        <td class="col-md-4"><a class="lbl" @click="select(row.id)">{{ row.label }}</a></td>
        <td class="col-md-1"><a class="remove" @click="remove(row.id)"><span class="remove glyphicon glyphicon-remove" aria-hidden="true"></span></a></td>
        <td class="col-md-6"></td>
      </tr>
    </tbody></table>
  </div>`,
}).mount("#main");
