// inventory: d_pac_e448_growth_product
var x;
parameters beta;
beta=.9;
model;
  x=.5*x(-1);
end;
pac_model(model_name=p,discount=beta,growth=x*x);
