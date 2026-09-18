// inventory: e222_dsge_var_missing_weight
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimation(dsge_var, datafile='d.csv');
