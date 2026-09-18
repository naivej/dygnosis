// inventory: e230_mh_tune_guess_alone
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimation(mh_tune_guess=0.2, datafile='d.csv');
