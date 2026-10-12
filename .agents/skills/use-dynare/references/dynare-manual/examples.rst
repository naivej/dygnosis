.. default-domain:: dynare

########
Examples
########

Dynare comes with a database of example ``.mod`` files, which are
designed to show a broad range of Dynare features, and are taken from
academic papers for most of them. They are distributed in the
``examples`` subdirectory.

Here is a short list of the examples included. For a more complete
description, please refer to the comments inside the files themselves.

Stochastic simulations
======================

``stochastic_simulations/collard_2001_theoretical_moments.mod``
``stochastic_simulations/collard_2001_simulated_moments.mod``

    Two examples of a small RBC model in a stochastic setup, presented
    in :cite:t:`Collard:2001` (see the file ``guide.pdf`` which comes with
    Dynare), both using :comm:`stoch_simul`. Both use numerical steady state
    computation. The first one uses theoretical moments, the second one uses
    simulated moments.

``stochastic_simulations/collard_2001_analytical_steady_state.mod``

    A small RBC model in a stochastic setup, presented in :cite:t:`Collard:2001`
    using :comm:`stoch_simul`. The steady state is solved analytically using the
    :bck:`steady_state_model` block and a helper function to call a solver.

``stochastic_simulations/schorfheide_2000_nonstationary.mod``

    The cash-in-advance model of :cite:t:`Schorfheide:2000` (same as ``estimation/schorfheide_2000.mod``) written in non-stationary
    form. Detrending of the equations is done by Dynare.

``stochastic_simulations/aguiar_gopinath_2007_trend.mod``

    Small open economy RBC model with shocks to the growth trend,
    presented in :cite:t:`Aguiar:2007`.

``stochastic_simulations/nk_baseline.mod``

    Baseline New Keynesian Model estimated in :cite:t:`FernandezVillaverde:2010`.
    It demonstrates how to use an explicit steady-state file
    to update parameters and call a numerical solver.

Estimation
==========

``estimation/schorfheide_2000.mod``

    A cash-in-advance model, estimated by :cite:t:`Schorfheide:2000`. The
    file shows how to use the :comm:`estimation` command.

``estimation/gali_2015.mod``

    Basic New Keynesian model of :cite:t:`Gali:2015`, Chapter 3 showing how to
    i) use "system prior"-type prior restrictions as in :cite:t:`Andrle:2018`
    and ii) run prior/posterior-functions.

``estimation/rbc_irf_matching.mod``

    Baseline RBC model with government spending shocks estimated via impulse response
    function (IRF) matching using :comm:`method_of_moments`.
    Both Frequentist (Maximum Likelihood) and Bayesian (Slice Sampling) approaches are presented.
    Additionally, it is shown how to estimate an AR(2)-process
    by working with the roots of the autoregressive process instead of the coefficients.

Perfect foresight
=================

``perfect_foresight/perfect_foresight_rbc.mod``

    An elementary real business cycle (RBC) model, simulated in a
    perfect foresight setup using :comm:`perfect_foresight_setup` and
    :comm:`perfect_foresight_solver`.

``perfect_foresight/perfect_foresight_expectation_errors.mod``

    Elementary RBC model (same as ``perfect_foresight/perfect_foresight_rbc.mod``), simulated in perfect
    foresight with expectation errors using
    :comm:`perfect_foresight_with_expectation_errors_setup` and
    :comm:`perfect_foresight_with_expectation_errors_solver`: agents behave as under
    perfect foresight, but they can still be surprised by unexpected shocks, and thus
    recompute their optimal plans when such an unexpected shock happens.

Optimal policy
==============

``optimal_policy/nk_ramsey_osr.mod``

    File demonstrating how to conduct optimal policy experiments in a
    simple New Keynesian model either under commitment
    (:comm:`ramsey_model`) or using
    optimal simple rules (:comm:`osr`). Based on
    :cite:t:`ChristianoMottoRostagno:2007`.

``optimal_policy/nk_ramsey_steady_file.mod``

    File demonstrating how to conduct optimal policy experiments in a
    simple New Keynesian model under commitment
    (:comm:`ramsey_model`) with a user-defined
    conditional steady state file. Based on
    :cite:t:`ChristianoMottoRostagno:2007`.

Heterogeneity
=============

``heterogeneity/krusell_smith_1998.mod``

    :cite:t:`Krusell:1998` model with heterogeneous households.
    Demonstrates loading a pre-computed steady state via
    :comm:`heterogeneity_load_steady_state`, solving the model with
    :comm:`heterogeneity_solve`, and computing impulse response functions.

``heterogeneity/krusell_smith_1998_steady_state.mod``

    :cite:t:`Krusell:1998` model with heterogeneous households.
    Demonstrates computing the steady state numerically using
    :comm:`heterogeneity_compute_steady_state`.

``heterogeneity/hank_one_asset.mod``

    One-asset HANK model. Demonstrates loading a pre-computed
    steady state via :comm:`heterogeneity_load_steady_state`, solving with
    :comm:`heterogeneity_solve`, and running stochastic simulations with
    :comm:`heterogeneity_simulate`.

``heterogeneity/hank_one_asset_steady_state.mod``

    One-asset HANK model. Demonstrates computing the steady state
    with parameter calibration using
    :comm:`heterogeneity_compute_steady_state`.

``heterogeneity/hank_two_assets.mod``

    Two-asset HANK model with liquid and illiquid assets. Demonstrates
    loading a pre-computed steady state via
    :comm:`heterogeneity_load_steady_state` and simulating news shocks.

``heterogeneity/hank_two_assets_steady_state.mod``

    Two-asset HANK model. Demonstrates computing the steady state
    with multi-parameter calibration using
    :comm:`heterogeneity_compute_steady_state`.

OccBin
======

``occbin/rbc_occbin.mod``

    RBC model with two occasionally binding constraints. Demonstrates
    how to set up OccBin using :comm:`occbin_setup` and :comm:`occbin_solver`.
    Based on :cite:t:`Guerrieri:2015`.

Macroprocessor
==============

``macroprocessor/bkk_1992.mod``

    Multi-country RBC model with time to build, presented in :cite:t:`Backus:1992`.
    The file shows how to use Dynare's macro processor.

Reporting
=========

``reporting/collard_2001_reporting.mod``

    Example of Dynare's reporting features using the :cite:t:`Collard:2001` RBC model.

Semistructural
==============

``semistructural/pac_model.mod``

    Example of a semi-structural model employing PAC (polynomial adjustment cost) specification.
