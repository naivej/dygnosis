// inventory: no E021 or W212 when unused varexo is removed unassigned
var y; varexo x;
model; y=0.5*y(-1); end;
initval; y=1; end;
var_remove x;
