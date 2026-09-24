// inventory: p_pac_e001_trends_string
var x;
parameters beta;
beta = .9;
model;
  x = x(-1);
end;

deterministic_trends;
  x ('beta');
end;
