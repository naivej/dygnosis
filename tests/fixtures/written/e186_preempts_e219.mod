// inventory: written_e186_preempts_e219 — original later-refusal fixture
var y dsge_prior_weight;
varexo e;
parameters rho;
rho = 0.5;
model;
y = rho * y(-1) + e;
end;
estimation(dsge_var=0.5, datafile='d.csv');
