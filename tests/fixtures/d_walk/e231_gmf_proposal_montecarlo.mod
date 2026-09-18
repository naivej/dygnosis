// inventory: e231_gmf_proposal_montecarlo
var y;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimation(filter_algorithm=gmf, proposal_approximation=montecarlo, datafile='d.csv');
