// inventory: d_pac_quiet_no_pac_model
var x;
varexo e;
parameters beta;
beta=.9;
model;
  x=.5*x(-1)+e;
end;
