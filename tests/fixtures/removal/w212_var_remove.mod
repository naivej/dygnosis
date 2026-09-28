// inventory: assignment before var_remove of an exogenous name
var y; varexo x;
model; y=0.5*y(-1); end;
initval; y=1; x=1; end;
var_remove x;
