// inventory: companions_named_missing
// Named catalog options and leftover quotes with no sibling files.
var y;
varexo e;
parameters rho;
rho = 0.5;

model;
y = rho * y(-1) + e;
end;

initval;
y = 0;
end;

estimation(datafile='missing_data.csv', mode_file=missing_mode);
data(file='missing_data_file.csv');
identification(gsa_sample_file=0);
identification(gsa_sample_file='missing_gsa.mat');
initval_file(filename='missing_initval.csv');
histval_file(filename='missing_histval.csv');
external_function(name=missing_ext, first_deriv_provided=missing_ext_d1);
prior_function(function=missing_prior);
run('missing_helper.m');
// estimation(datafile='commented_data.csv');
// run('commented_helper.m');
