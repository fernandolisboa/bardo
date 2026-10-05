---
id: connect-instagram
title: Conectar o Instagram
group: publishing
---

# Conectar o Instagram

O Bardo entra no Instagram por um app da Meta que você registra na sua própria conta de desenvolvedor da Meta, com a API do Instagram com login do Facebook para Empresas. O Bardo não traz app próprio, então o app, o nível de acesso dele e qualquer revisão são seus. Este guia configura o app uma vez; depois disso, cada conta do Instagram conecta pelo cartão da conta de rede.

<a id="need"></a>
## Do que você precisa

- Uma conta **profissional** do Instagram (Empresa ou Criador de conteúdo).
- Uma **Página do Facebook** vinculada a essa conta do Instagram. O envio de um arquivo de vídeo local só é documentado para apps que usam o Login do Facebook para Empresas, e esse login chega ao Instagram pela Página vinculada.
- Uma conta do Facebook com uma função na Página (que possa criar conteúdo nela) e no app da Meta abaixo.
- Acesso ao [Meta for Developers](https://developers.facebook.com/apps/).

Para vincular a conta a uma Página: no Instagram, abra **Configurações › Central de Contas** (ou **Editar perfil › Página**) e conecte a Página, ou faça isso nas configurações da Página no Meta Business Suite.

<a id="app"></a>
## 1. Crie o app da Meta

1. Em **Meus apps**, escolha **Criar app**.
2. Caso de uso: **Gerenciar mensagens e conteúdo no Instagram** (o caso de uso do Instagram). Tipo de app, se for pedido: **Empresa**. Não use o tipo Nativo ou Computador: o Login do Facebook para Empresas precisa de um login web.
3. Nas configurações do caso de uso, abra **Configuração da API com login do Facebook** e adicione o **Login do Facebook para Empresas** se ele ainda não estiver lá.
4. Em **Configurações do app › Básico**, copie o **ID do app** (um número) e a **Chave secreta do app** (escolha **Mostrar**).

O app fica no modo **Desenvolvimento** com **Acesso padrão**: isso atende toda conta cujo usuário do Facebook tem uma função no app, que é você. Nenhuma Análise do app nem Verificação da empresa é necessária para as suas próprias contas.

Os nomes dos menus podem aparecer em inglês, conforme o idioma da sua conta: *My Apps*, *Create app*, *App settings › Basic*, *Development*, *Standard Access*.

<a id="save"></a>
## 2. Salve o app no Bardo

1. Abra [Configurações › Redes](bardo:go/settings/networks).
2. Cole o ID e a chave secreta do app em **Instagram · app da Meta** e escolha **Salvar**.

Os dois ficam no Gerenciador de Credenciais do Windows, na sua conta do Windows, por perfil do Bardo. Eles nunca chegam ao banco de dados, aos logs nem às mensagens de erro do Bardo, e a chave secreta não aparece mais na tela; o cartão mostra os quatro últimos caracteres dela.

<a id="token"></a>
## 3. Gere um token

O Login do Facebook não devolve o login a um app para computador pelo navegador, como o Google faz: ele só volta a um endereço HTTPS exato ou a uma visualização web embutida. Então você entra uma vez no Explorador da Graph API da Meta e cola no Bardo o token que ele dá.

1. Abra o [Explorador da Graph API](https://developers.facebook.com/tools/explorer/) (o botão **Abrir o Explorador da Graph API** do cartão da conta leva até lá).
2. Em **App da Meta**, escolha o seu app. Em **Usuário ou Página**, escolha **Obter token de acesso do usuário**.
3. Marque estas permissões:
   - `instagram_basic`
   - `instagram_content_publish`
   - `instagram_manage_insights`
   - `pages_show_list`
   - `pages_read_engagement`

   Se a Página pertence a um portfólio empresarial (Meta Business Suite) e a sua função nela vem de lá, marque também `business_management`, `ads_management` e `ads_read`.
4. Escolha **Gerar token de acesso**, entre e, na janela, escolha a Página e a conta do Instagram que o Bardo pode usar. Permita todas as permissões.
5. Copie o token do campo **Token de acesso**.

O token dura cerca de uma hora, então cole no Bardo logo em seguida.

<a id="connect"></a>
## 4. Conecte a conta

1. Abra [Contas](bardo:go/accounts), escolha o canal e adicione (ou abra) a conta do Instagram Reels dele.
2. Escolha **Conectar**, cole o token e escolha **Conectar** de novo.
3. O Bardo troca o token por um de longa duração, confere se todas as permissões foram dadas e encontra as Páginas que ele alcança com a conta do Instagram vinculada:
   - uma conta: o cartão mostra **Conectada como @usuário**;
   - várias: o cartão lista cada uma com a Página dela; escolha a deste canal com **Conectar esta**;
   - nenhuma Página, ou nenhuma Página com conta profissional do Instagram vinculada: o cartão diz qual dos dois, e nada é guardado.

O Bardo nunca guarda o token que você colou. Ele guarda, no Gerenciador de Credenciais do Windows, por perfil e por conta de rede, o token de usuário de longa duração e o token da Página escolhida, que é o que publica. O banco de dados do Bardo guarda só o id e o nome de usuário da conta do Instagram, as permissões, a validade e a última renovação.

<a id="day-to-day"></a>
## No dia a dia

- **O token de longa duração dura cerca de 60 dias.** Quando falta uma semana para ele expirar, o Bardo o troca por um novo, e lê de novo o token da Página, na próxima vez que você abrir o Bardo ou escolher **Verificar**. Se o Bardo não for aberto nessa última semana, o token expira e o cartão pede que você reconecte com um token novo.
- **Verificar** faz essa renovação se ela estiver na hora e lê de novo o nome de usuário da conta. Se a Página estiver vinculada agora a outra conta do Instagram, o cartão passa a **Reconexão necessária**.
- **Reconexão necessária** quer dizer que a Meta recusou o token: ele expirou, foi revogado (remover o app nas configurações do Facebook faz isso), sua senha mudou, a Meta pediu que você autorize o app de novo (o acesso a dados expira 90 dias depois do seu último uso dele num login da Meta), ou a Página não está mais vinculada. Gere um token novo como no passo 3 e escolha **Reconectar**.
- **Desconectar** remove o acesso do Bardo na Meta e esquece os tokens. A Meta revoga o acesso de um app para a conta do Facebook inteira de uma vez, então, quando outra conta no mesmo perfil do Bardo ainda está conectada pelo mesmo app, o Bardo esquece os tokens desta conta, deixa o acesso como está e avisa. Se não for possível falar com a Meta, o Bardo esquece os tokens mesmo assim e pede que você remova o app em **Configurações e privacidade › Integrações comerciais** na sua conta do Facebook.
- Uma conta conectada precisa ser desconectada antes de ser removida.

<a id="upload"></a>
## Enviar um Reel

Na etapa Publicação, uma conta conectada do Instagram Reels oferece **Revisar envio** quando o preset dela está renderizado e a legenda escrita.

- **Requisitos do Reel.** Antes de a revisão abrir, o Bardo confere o arquivo renderizado contra o que o Instagram aceita como Reel: MP4 ou MOV em fast start, H.264 ou HEVC, de 23 a 60 quadros por segundo, até 1920 pixels de largura, de 3 segundos a 15 minutos e até 300 MB. Cada requisito não atendido aparece embaixo do envio, e a revisão fica fechada até um render novo passar.
- **A revisão** mostra o arquivo, a conta e a legenda com as hashtags do jeito que o Instagram as recebe. Você escolhe a **capa** (o quadro nesse tempo, em segundos ou m:ss), **Mostrar também no Feed** (ligado: o Reel aparece também na sua grade e no feed dos seguidores) e o **rótulo "Informações de IA"**, ligado quando a narração usou uma voz realista.
- **Enviar e publicar** manda o arquivo e, quando o Instagram termina de processar, publica o Reel. O Bardo verifica a cada minuto; se o Instagram ainda estiver processando depois de uns quinze minutos, o post mostra **Ainda processando** e **Verificar de novo** retoma mais tarde.
- **Limite de publicação.** O Instagram deixa uma conta publicar um certo número de posts por apps em 24 horas (o Bardo lê o número do Instagram). Acima dele, o Reel fica na fila e o post diz quando ele sai. Quando outros apps também publicaram na conta, o Bardo não sabe quando os posts deles saem da janela, então diz quando vai ler o limite de novo (dentro de uma hora). Parar e retomar lê o limite de novo.
- **Parar e retomar.** Um envio parado ou interrompido retoma do que o Instagram já tem. Um contêiner que fica 24 horas sem publicar expira no Instagram; o Bardo então manda o arquivo de novo num contêiner novo, no máximo duas vezes.
- Se o Instagram publicar o Reel com um aviso (por exemplo, que deixou o áudio de fora), o post mostra o aviso do Instagram.
- Se a conta for reconectada como outra conta do Instagram antes de o Reel sair, o envio para e pede uma nova revisão.

<a id="schedule"></a>
## Agendar um Reel

Na revisão, **Quando** oferece **Assim que processar** ou **Agendar**. O Instagram não aceita horário de publicação de apps, então com **Agendar** o Bardo manda o arquivo antes e publica o Reel ele mesmo na data e hora que você digitar (lidas no fuso horário do seu computador, que a revisão mostra). O post mostra **Agendado no Bardo** com o horário em que vai sair, e **Agendar no Bardo** confirma a revisão.

Deixe o Bardo aberto nesse horário e o computador ligado, ou ligue o [agente em segundo plano](background-agent.md), que envia com o Bardo fechado enquanto você estiver conectado ao Windows. Se nenhum dos dois estiver rodando ou o computador estiver desligado, nada é postado: na próxima vez que o Bardo abrir, ele mostra o post entre os [posts perdidos](missed-posts.md), para você postar agora, dar um novo horário ou cancelar. Um horário que já passou é recusado quando você confirma.

<a id="metrics"></a>
## Números na tela Desempenho

Com a conta conectada, toda sincronização de métricas lê os insights dos Reels do canal: os que o Bardo publicou e os posts vinculados com **Marcar como publicado**. Eles não precisam de chave do YouTube.

- **O que ela lê:** visualizações, alcance, curtidas, comentários, compartilhamentos, salvamentos, interações e, num Reel, o tempo de exibição médio e total. O Instagram não informa receita.
- **Dados atrasados.** Os números do Instagram chegam até dois dias depois do post. Até lá, o post diz que ainda não tem números, e um número que o Instagram deixa de fora aparece como "—", não 0.
- **Posts vinculados.** Um link colado traz um código curto, não o id que os insights usam, então a primeira sincronização procura o post nas mídias da conta (as 2.000 mais recentes) e guarda o id dele. Um post vinculado que a conta não tem aparece como **Não encontrado**.
- **Custo.** Cerca de uma requisição por Reel por sincronização (duas para um post do feed), mais a lista de mídias para um post vinculado até a sincronização encontrá-lo ali.
- Se o Instagram não mostrar os insights de um post (ele os segura em posts com poucos espectadores), o post mantém o link e espera a próxima sincronização; ele não é marcado como **Não encontrado**.
- Sem conta conectada, um post vinculado guarda só o link. Quando a conta precisa reconectar, as sincronizações pulam os posts dela até a reconexão.

<a id="authorization"></a>
## Se a Página pedir autorização de publicação

A Meta pode pedir que os administradores de uma Página concluam a **Autorização de publicação da Página** (uma verificação de identidade nas configurações da Página) antes de qualquer coisa ser publicada por ela. Se um envio for recusado por esse motivo, conclua a autorização nas configurações da Página e tente de novo.
