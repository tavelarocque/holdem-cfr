# Reading list, by milestone

Compiled October 2026. Each entry says which part of this project it strengthens and what to take from it.

## Read these first

1. Tammelin, Burch, Johanson & Bowling (2015). *Solving Heads-up Limit Texas Hold'em.* IJCAI. https://www.ijcai.org/Abstract/15/097. Read it with Tammelin (2014), *Solving Large Imperfect Information Games Using CFR+*, https://arxiv.org/abs/1407.5042. Together they are the blueprint for this exact game: RM+, alternating updates, delayed linear averaging, and compressed regret storage.
2. Johanson, Waugh, Bowling & Zinkevich (2011). *Accelerating Best Response Calculation in Large Extensive Games.* IJCAI. https://ijcai.org/Abstract/11/054. Shows how to traverse the public tree carrying vectors of ranges, how to get an O(n) showdown by sorting hands by strength and using running sums, and how to correct for card removal. It drives both exploitability measurement and fast CFR on hold'em.
3. Brown & Sandholm (2019). *Solving Imperfect-Information Games via Discounted Regret Minimization.* AAAI. https://arxiv.org/abs/1809.04040. Introduces DCFR (1.5, 0, 2) and Linear CFR. Already implemented here.
4. Neller & Lanctot (2013). *An Introduction to Counterfactual Regret Minimization.* https://modelai.gettysburg.edu/2013/cfr/index.html
5. Lanctot, Waugh, Zinkevich & Bowling (2009). *Monte Carlo Sampling for Regret Minimization in Extensive Games.* NeurIPS. https://proceedings.neurips.cc/paper/2009/hash/00411460f7c92d2124a67ea0f4cb5f85-Abstract.html
6. Johanson, Burch, Valenzano & Bowling (2013). *Evaluating State-Space Abstractions in Extensive-Form Games.* AAMAS. https://webdocs.cs.ualberta.ca/~bowling/papers/13aamas-abstraction.pdf. Read it with Ganzfried & Sandholm (2014), *Potential-Aware Imperfect-Recall Abstraction with Earth Mover's Distance*, AAAI, https://ojs.aaai.org/index.php/AAAI/article/view/8816.
7. Brown & Sandholm (2017). *Safe and Nested Subgame Solving for Imperfect-Information Games.* NeurIPS. https://arxiv.org/abs/1705.02955. Read it with Burch, Johanson & Bowling (2014), *Solving Imperfect Information Games Using Decomposition (CFR-D)*, https://arxiv.org/abs/1303.4441.
8. Burch et al. (2018). *AIVAT: A New Variance Reduction Technique for Agent Evaluation.* AAAI. https://arxiv.org/abs/1612.06915

## M1–M2: Engine (done)

- Zinkevich, Johanson, Bowling & Piccione (2007). *Regret Minimization in Games with Incomplete Information.* NeurIPS. https://proceedings.neurips.cc/paper/2007/hash/08d98638c6fcd194a4b1e6992063e944-Abstract.html
- Kuhn (1950). *A Simplified Two-Person Poker.* https://sites.math.rutgers.edu/~zeilberg/akherim/PokerPapers/Kuhn1951.pdf. Gives the closed-form equilibrium family used in `games/tests/kuhn.rs`.
- Southey et al. (2005). *Bayes' Bluff: Opponent Modelling in Poker.* UAI. https://arxiv.org/abs/1207.1411. The source of Leduc hold'em.
- Burch, Moravčík & Schmid (2019). *Revisiting CFR+ and Alternating Updates.* JAIR. https://arxiv.org/abs/1810.11542
- Hart & Mas-Colell (2000). *A Simple Adaptive Procedure Leading to Correlated Equilibrium.* Econometrica. https://math.huji.ac.il/hart/abs/adapt.html. Regret matching's origin in economics.
- Lanctot et al. (2019). *OpenSpiel.* https://arxiv.org/abs/1908.09453. Its reference CFR and exploitability numbers can serve as a cross-check.
- Upgrades to try later:
  - Farina, Kroer & Sandholm (2021). *Predictive CFR+.* AAAI. https://arxiv.org/abs/2007.14358
  - Zhang, McAleer & Sandholm (2024). *Faster Game Solving via Hyperparameter Schedules.* https://arxiv.org/abs/2404.09097
  - Xu et al. (2024). *PDCFR+.* IJCAI. https://www.ijcai.org/proceedings/2024/583

## M3: Hand evaluator

- Henry R. Lee, PH Evaluator (perfect hash). https://github.com/HenryRLee/PokerHandEvaluator/blob/develop/Documentation/Algorithm.md. The design used in `crates/eval`: a perfect hash over rank counts plus a flush table.
- Cactus Kev, *Poker Hand Evaluator* and the 7,462 equivalence classes. http://suffe.cool/poker/7462.html
- TwoPlusTwo evaluator, a roughly 120 MB state machine. https://bostik.iki.fi/aivoituksia/projects/twoplustwo-hand-evaluator.html
- Practical note: inside CFR and best response, rank all 1,326 hole-card pairs once per river board and sort them. After that, raw evaluator speed mostly matters for building the abstraction.

## M4: Abstraction and hand indexing

- Waugh (2013). *A Fast and Optimal Hand Isomorphism Algorithm.* https://www.cs.cmu.edu/~waugh/publications/isomorphism13.pdf. Implement this before bucketing.
- Johanson (2007). *Robust Strategies and Counter-Strategies* (MSc thesis), section 2.5.5, which defines E[HS] and E[HS²]. https://era.library.ualberta.ca/items/6d1ece9c-ffc0-4327-a838-bb83fb6fcaae/download/3370f841-20f7-4b96-ada0-c8a5ce8efd0a
- Gilpin, Sandholm & Sørensen (2007). *Potential-Aware Automated Abstraction.* AAAI. https://mlanthology.org/aaai/2007/gilpin2007aaai-potential
- Waugh et al. (2009). *Abstraction Pathologies in Extensive Games.* AAMAS. https://www.cs.cmu.edu/~waugh/publications/aamas09.pdf. Shows that a finer abstraction can be more exploitable.
- Waugh et al. (2009). *A Practical Use of Imperfect Recall.* https://www.cs.cmu.edu/~waugh/publications/sara09.pdf
- Lanctot et al. (2012). *No-Regret Learning in Extensive-Form Games with Imperfect Recall.* ICML. https://arxiv.org/abs/1205.0622
- Johanson, Bard, Burch & Bowling (2012). *CFR-BR.* AAAI. https://ojs.aaai.org/index.php/AAAI/article/view/8269
- Fu et al. (2025). *Beyond Outcome-Based Imperfect-Recall.* https://arxiv.org/abs/2510.15094
- Li & Huang (2026). *WEVA: Effective, Efficient, and General Information Abstraction.* https://arxiv.org/abs/2605.10900. Clusters hands on per-hand expected values taken from a few warm-up iterations of CFR. A cheap novel experiment.

## M5: Monte Carlo CFR

- Johanson et al. (2012). *Efficient Nash Equilibrium Approximation through Monte Carlo CFR* (public chance sampling). AAMAS. https://webdocs.cs.ualberta.ca/~bowling/papers/12aamas-pcs.pdf. Benchmark it against external sampling.
- Gibson (2014). PhD thesis, which covers Pure CFR. https://www.cs.ualberta.ca/~games/poker/publications/gibson.phd.pdf
- Brown & Sandholm (2015, 2017). Regret-based pruning. https://proceedings.mlr.press/v70/brown17a.html
- Brown & Sandholm (2019). *Superhuman AI for multiplayer poker* (Pluribus). Science. https://par.nsf.gov/servlets/purl/10119653. Source of the rule that skips actions with very negative regret in 95% of iterations, and of Linear MCCFR.
- Schmid et al. (2019). *VR-MCCFR.* AAAI. https://arxiv.org/abs/1809.03057
- Lisý & Bowling (2017). *Local Best Response.* https://arxiv.org/abs/1612.07547

## M6: Subgame solving

- Moravčík et al. (2016). *Refining Subgames* (max-margin). AAAI. https://ojs.aaai.org/index.php/AAAI/article/view/10033
- Ganzfried & Sandholm (2015). *Endgame Solving.* AAMAS. https://aamas.csc.liv.ac.uk/Proceedings/aamas2015/aamas/p37.pdf. Use it as the unsafe baseline.
- Brown, Sandholm & Amos (2018). *Depth-Limited Solving.* https://arxiv.org/abs/1805.08195
- DeepStack (2017), https://arxiv.org/abs/1701.01724. Libratus (2018), https://par.nsf.gov/biblio/10077416. ReBeL (2020), https://arxiv.org/abs/2007.13544.
- Li & Huang (2026). *Real-Time Parallel CFR.* https://arxiv.org/abs/2605.19928

## M7: Evaluation

- Bowling et al. (2008). *Strategy Evaluation in Extensive Games with Importance Sampling.* ICML. https://webdocs.cs.ualberta.ca/~bowling/publications/b2hd-08icml-ispoker.html
- Bard et al. (2013). *Online Implicit Agent Modelling.* AAMAS. https://webdocs.cs.ualberta.ca/~bowling/publications/b2hd-13aamas-implicitmodelling.html

## Stretch goals

- Kroer & Sandholm (2018). *A Unified Framework for Extensive-Form Game Abstraction with Bounds.* NeurIPS. https://papers.neurips.cc/paper_files/paper/2018/file/aa942ab2bfa6ebda4840e7360ce6e7ef-Paper.pdf
- Ganzfried & Sandholm (2013). *Action Translation.* IJCAI. https://www.ijcai.org/Abstract/13/028
- Johanson (2013). *Measuring the Size of Large No-Limit Poker Games.* https://arxiv.org/abs/1302.7008

## SSRN: finance and economics context

SSRN has almost nothing on CFR engineering. These papers are useful for framing the project, not for writing code.

- Kovařík et al. (2024). *Revisiting Game Representations.* https://papers.ssrn.com/sol3/papers.cfm?abstract_id=4690803. Argues for a public tree plus private types, which is the hold'em structure.
- Lomys & Magnolfi. *Estimation of Games under No Regret.* https://papers.ssrn.com/sol3/papers.cfm?abstract_id=4269273
- Balseiro & Gur (2017). *Learning in Repeated Auctions with Budgets.* https://papers.ssrn.com/sol3/papers.cfm?abstract_id=2921446
- Cartea, Chang & Penalva. *Algorithms and Supracompetitive Prices: The Impact of Tick Size.* https://papers.ssrn.com/sol3/papers.cfm?abstract_id=4105954
- Macrì & Lillo. *Deviations from Nash in a Two-Player Optimal Execution Game with RL.* https://papers.ssrn.com/sol3/papers.cfm?abstract_id=4932871
- Lu, Mortal & Ray. *Hedge Fund Hold 'em.* https://papers.ssrn.com/sol3/papers.cfm?abstract_id=3392130
- Levitt & Miles (2011). *The Role of Skill Versus Luck in Poker.* https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1833160
- Abramowicz (2020). *Modeling Settlement Bargaining with Algorithmic Game Theory* (CFR applied to law and economics). https://papers.ssrn.com/sol3/papers.cfm?abstract_id=3709964
