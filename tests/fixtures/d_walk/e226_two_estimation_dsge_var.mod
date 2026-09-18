// inventory: e226_two_estimation_dsge_var
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimation(dsge_var, datafile='d.csv');
estimation(dsge_var=0.5, datafile='d.csv');
