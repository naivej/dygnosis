// inventory: e271_option_twice
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
varobs y;
estimation(dsge_var, dsge_var=0.5, datafile='d.csv');
