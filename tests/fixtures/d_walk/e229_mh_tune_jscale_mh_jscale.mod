// inventory: e229_mh_tune_jscale_mh_jscale
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimation(mh_tune_jscale, mh_jscale=0.4, datafile='d.csv');
